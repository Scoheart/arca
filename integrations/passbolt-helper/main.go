// arca-passbolt bridges Arca's bounded stdin/stdout protocol to the Passbolt SDK.
package main

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/google/uuid"
	"github.com/passbolt/go-passbolt/api"
)

const (
	protocolVersion = 1
	maxInput        = 512 * 1024
	maxKeyFile      = 256 * 1024
	maxAPIResponse  = 16 * 1024 * 1024
	maxPassword     = 4096
)

type request struct {
	Protocol          int    `json:"protocol"`
	Operation         string `json:"operation"`
	ServerURL         string `json:"server_url"`
	ServerFingerprint string `json:"server_fingerprint"`
	UserID            string `json:"user_id"`
	FolderID          string `json:"folder_id"`
	PrivateKeyPath    string `json:"private_key_path"`
	Password          string `json:"password"`
	Revision          string `json:"revision"`
	Locator           string `json:"locator"`
	Key               string `json:"key"`
}

type reply struct {
	Protocol int    `json:"protocol"`
	Status   string `json:"status"`
	Locator  string `json:"locator,omitempty"`
	Secret   []byte `json:"secret,omitempty"`
}

const (
	ok                    = "ok"
	invalidRequest        = "invalid-request"
	keyFile               = "key-file"
	serverKey             = "server-key"
	authentication        = "authentication"
	privateFolder         = "private-folder"
	unsupportedFormat     = "unsupported-format"
	requestFailed         = "request-failed"
	alreadyExists         = "already-exists"
	ambiguous             = "ambiguous"
	unowned               = "unowned"
	missing               = "missing"
	createUnconfirmed     = "create-unconfirmed"
	retirementUnconfirmed = "retirement-unconfirmed"
)

func main() {
	result := readAndExecute()
	result.Protocol = protocolVersion
	// The protocol returns fixed statuses. SDK diagnostics may contain server
	// data, so they never go to stdout or stderr.
	err := json.NewEncoder(os.Stdout).Encode(result)
	clear(result.Secret)
	if err != nil {
		os.Exit(1)
	}
}

func readAndExecute() reply {
	if len(os.Args) != 1 {
		return reply{Status: invalidRequest}
	}
	var req request
	dec := json.NewDecoder(http.MaxBytesReader(nil, os.Stdin, maxInput))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&req); err != nil || dec.Decode(new(any)) != io.EOF {
		return reply{Status: invalidRequest}
	}
	server, err := url.Parse(req.ServerURL)
	if err != nil || !validRequest(req, server) {
		return reply{Status: invalidRequest}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 24*time.Second)
	defer cancel()
	return execute(ctx, req, server)
}

func validRequest(req request, server *url.URL) bool {
	if req.Protocol != protocolVersion || !isID(req.UserID) ||
		(req.FolderID != "" && !isID(req.FolderID)) || !filepath.IsAbs(req.PrivateKeyPath) ||
		req.Password == "" || len(req.Password) > 64*1024 || server.Scheme != "https" ||
		server.Hostname() == "" || server.User != nil || server.RawQuery != "" ||
		server.ForceQuery || strings.Contains(req.ServerURL, "#") {
		return false
	}
	digest, err := hex.DecodeString(req.ServerFingerprint)
	if err != nil || (len(digest) != 20 && len(digest) != 32) {
		return false
	}
	switch req.Operation {
	case "check":
		return req.Revision == "" && req.Locator == "" && req.Key == ""
	case "recover":
		return isID(req.Revision) && req.Locator == "" && req.Key == ""
	case "create":
		return isID(req.Revision) && req.Locator == "" && validKey(req.Key)
	case "read", "retire":
		return isID(req.Revision) && isID(req.Locator) && req.Key == ""
	}
	return false
}

func isID(value string) bool {
	parsed, err := uuid.Parse(value)
	return err == nil && parsed != uuid.Nil && parsed.String() == value
}

func validKey(value string) bool {
	return value != "" && !strings.ContainsAny(value, "\x00\r\n") &&
		utf8.RuneCountInString(value) <= maxPassword
}

func execute(ctx context.Context, req request, server *url.URL) reply {
	privateKey, err := readPrivateKey(req.PrivateKeyPath)
	if err != nil {
		return reply{Status: keyFile}
	}
	defer clear(privateKey)
	transport, err := newTransport(server)
	if err != nil {
		return reply{Status: serverKey}
	}
	httpClient := &http.Client{Transport: transport, Timeout: 10 * time.Second}
	defer transport.base.CloseIdleConnections()
	client, err := api.NewClient(httpClient, "Arca Passbolt integration", req.ServerURL, string(privateKey), req.Password)
	if err != nil {
		return reply{Status: authentication}
	}
	if err := verifyServer(ctx, client, req.ServerFingerprint); err != nil {
		return reply{Status: serverKey}
	}
	err = client.Login(ctx)
	if client.GetUserID() != "" {
		defer func() {
			logoutCtx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			_ = client.Logout(logoutCtx)
		}()
	}
	if err != nil || client.GetUserID() != req.UserID {
		return reply{Status: authentication}
	}
	store := vault{client: client, userID: req.UserID, folderID: req.FolderID}
	if !store.checkFolder(ctx) {
		return reply{Status: privateFolder}
	}
	switch req.Operation {
	case "check":
		_, status := store.resourceType(ctx)
		return reply{Status: status}
	case "recover":
		return store.recover(ctx, req.Revision)
	case "create":
		return store.create(ctx, req.Revision, req.Key, transport)
	case "read", "retire":
		return store.byID(ctx, req)
	}
	return reply{Status: invalidRequest}
}

func readPrivateKey(filename string) ([]byte, error) {
	info, err := os.Lstat(filename)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0o077 != 0 {
		return nil, errors.New("private key must be a private regular file")
	}
	file, err := os.Open(filename)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) || opened.Mode().Perm()&0o077 != 0 {
		return nil, errors.New("private key file changed while opening")
	}
	data, err := io.ReadAll(io.LimitReader(file, maxKeyFile+1))
	if err != nil || len(data) == 0 || len(data) > maxKeyFile {
		clear(data)
		return nil, errors.New("private key exceeds the allowed size or cannot be read")
	}
	return data, nil
}

func verifyServer(ctx context.Context, client *api.Client, expectedFingerprint string) error {
	key, fingerprint, err := client.GetPublicKey(ctx)
	if err != nil || !strings.EqualFold(fingerprint, expectedFingerprint) {
		return errors.New("Passbolt server key does not match")
	}
	id, err := uuid.NewRandom()
	if err != nil {
		return err
	}
	token := "gpgauthv1.3.0|36|" + id.String() + "|gpgauthv1.3.0"
	ciphertext, err := client.EncryptMessageWithPublicKey(key, token)
	if err != nil {
		return err
	}
	return client.VerifyServer(ctx, token, ciphertext)
}

type boundedTransport struct {
	base            *http.Transport
	serverHost      string
	createPath      string
	createAttempted bool
}

func newTransport(server *url.URL) (*boundedTransport, error) {
	base := http.DefaultTransport.(*http.Transport).Clone()
	if certPath := os.Getenv("SSL_CERT_FILE"); certPath != "" {
		roots, err := x509.SystemCertPool()
		if err != nil {
			return nil, err
		}
		pem, err := os.ReadFile(certPath)
		if err != nil || !roots.AppendCertsFromPEM(pem) {
			return nil, errors.New("invalid HTTPS trust certificate file")
		}
		base.TLSClientConfig = &tls.Config{RootCAs: roots}
	}
	return &boundedTransport{
		base:       base,
		serverHost: server.Host,
		createPath: path.Join(server.Path, "/resources.json"),
	}, nil
}

func (t *boundedTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	if req.URL.Scheme != "https" || req.URL.Host != t.serverHost {
		return nil, errors.New("request left the configured Passbolt server")
	}
	if req.Method == http.MethodPost && req.URL.Path == t.createPath {
		t.createAttempted = true
	}
	resp, err := t.base.RoundTrip(req)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode >= 300 && resp.StatusCode < 400 {
		resp.Body.Close()
		return nil, errors.New("unexpected Passbolt redirect")
	}
	resp.Body = http.MaxBytesReader(nil, resp.Body, maxAPIResponse)
	return resp, nil
}
