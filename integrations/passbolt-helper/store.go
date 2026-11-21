package main

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"

	"github.com/ProtonMail/gopenpgp/v3/crypto"
	"github.com/passbolt/go-passbolt/api"
	"github.com/passbolt/go-passbolt/helper"
)

const (
	v4Resource = "password-and-description"
	v5Resource = "v5-default"
)

type vault struct {
	client   *api.Client
	userID   string
	folderID string
}

func (v vault) checkFolder(ctx context.Context) bool {
	if v.folderID == "" {
		return true
	}
	folder, err := v.client.GetFolder(ctx, v.folderID, &api.GetFolderOptions{ContainPermissions: true})
	return err == nil && folder.ID == v.folderID && folder.Personal &&
		v.isPrivate(folder.Permissions, "Folder", folder.ID)
}

func (v vault) isPrivate(permissions []api.Permission, kind, id string) bool {
	if len(permissions) != 1 {
		return false
	}
	permission := permissions[0]
	return permission.Type == 15 && permission.ACO == kind && permission.ACOForeignKey == id &&
		permission.ARO == "User" && permission.AROForeignKey == v.userID
}

func (v vault) resourceType(ctx context.Context) (string, string) {
	types, err := v.client.GetResourceTypes(ctx, nil)
	if err != nil {
		return "", requestFailed
	}
	settings := v.client.MetadataTypeSettings()
	allowV5 := settings.AllowCreationOfV5Resources && v.client.MetadataKeySettings().AllowUsageOfPersonalKeys
	var v4, v5 bool
	for _, resourceType := range types {
		v4 = v4 || resourceType.Slug == v4Resource
		v5 = v5 || resourceType.Slug == v5Resource
	}
	if v5 && allowV5 && settings.DefaultResourceType == api.PassboltAPIVersionTypeV5 {
		return v5Resource, ok
	}
	if v4 && settings.AllowCreationOfV4Resources {
		return v4Resource, ok
	}
	if v5 && allowV5 {
		return v5Resource, ok
	}
	return "", unsupportedFormat
}

func (v vault) create(ctx context.Context, revision, key string, transport *boundedTransport) reply {
	slug, status := v.resourceType(ctx)
	if status != ok {
		return reply{Status: status}
	}
	resource, status := v.find(ctx, revision)
	if status != ok {
		return reply{Status: status}
	}
	if resource != nil {
		return reply{Status: alreadyExists}
	}
	id, err := helper.CreateResourceGeneric(ctx, v.client, slug, v.folderID,
		map[string]any{"name": title(revision)},
		map[string]any{"password": key, "description": marker(revision)},
	)
	if err != nil || !isID(id) {
		if transport.createAttempted {
			return reply{Status: createUnconfirmed}
		}
		return reply{Status: requestFailed}
	}
	return reply{Status: ok, Locator: id}
}

func (v vault) recover(ctx context.Context, revision string) reply {
	resource, status := v.find(ctx, revision)
	if status != ok || resource == nil {
		return reply{Status: status}
	}
	key, status := v.ownedKey(ctx, resource, revision)
	clear(key)
	if status != ok {
		return reply{Status: status}
	}
	return reply{Status: ok, Locator: resource.ID}
}

func (v vault) find(ctx context.Context, revision string) (*api.Resource, string) {
	opts := api.GetResourcesOptions{FilterIsOwnedByMe: true, ContainResourceType: true}
	if v.folderID != "" {
		opts.FilterHasParent = []string{v.folderID}
	}
	items, err := v.client.GetResources(ctx, &opts)
	if err != nil {
		return nil, requestFailed
	}
	var matched *api.Resource
	for i := range items {
		item := &items[i]
		if item.FolderParentID != v.folderID || item.Deleted {
			continue
		}
		name := item.Name
		if item.Metadata != "" {
			// The bridge creates v5 metadata using the account's own key.
			if item.MetadataKeyType != api.MetadataKeyTypeUserKey || item.CreatedBy != v.userID {
				continue
			}
			if item.ResourceType.ID == "" || item.ResourceType.ID != item.ResourceTypeID {
				return nil, requestFailed
			}
			if item.ResourceType.Slug != v5Resource {
				continue
			}
			name, err = v.name(ctx, item, &item.ResourceType)
			if err != nil {
				return nil, requestFailed
			}
		}
		if name == title(revision) {
			if matched != nil {
				return nil, ambiguous
			}
			matched = item
		}
	}
	return matched, ok
}

func (v vault) byID(ctx context.Context, req request) reply {
	resource, err := v.client.GetResource(ctx, req.Locator)
	if err != nil {
		var apiError *api.APIError
		if errors.As(err, &apiError) && apiError.StatusCode == http.StatusNotFound {
			if req.Operation == "retire" {
				// CE hides both deleted and inaccessible resources.
				return reply{Status: retirementUnconfirmed}
			}
			return reply{Status: missing}
		}
		return reply{Status: requestFailed}
	}
	if resource.ID != req.Locator {
		return reply{Status: unowned}
	}
	key, status := v.ownedKey(ctx, resource, req.Revision)
	if status != ok {
		return reply{Status: status}
	}
	if req.Operation == "read" {
		return reply{Status: ok, Secret: key}
	}
	clear(key)
	if err := v.client.DeleteResource(ctx, req.Locator); err != nil {
		return reply{Status: retirementUnconfirmed}
	}
	return reply{Status: ok}
}

func (v vault) ownedKey(ctx context.Context, resource *api.Resource, revision string) ([]byte, string) {
	if !isID(resource.ID) || resource.Deleted || resource.FolderParentID != v.folderID || resource.CreatedBy != v.userID {
		return nil, unowned
	}
	resourceType, err := v.client.GetResourceType(ctx, resource.ResourceTypeID)
	if err != nil {
		return nil, requestFailed
	}
	v4 := resourceType.Slug == v4Resource && resource.Metadata == ""
	v5 := resourceType.Slug == v5Resource && resource.Metadata != "" && resource.MetadataKeyType == api.MetadataKeyTypeUserKey
	if resourceType.ID != resource.ResourceTypeID || (!v4 && !v5) {
		return nil, unowned
	}
	name, err := v.name(ctx, resource, resourceType)
	if err != nil {
		return nil, requestFailed
	}
	if name != title(revision) {
		return nil, unowned
	}
	permissions, err := v.client.GetResourcePermissions(ctx, resource.ID)
	if err != nil {
		return nil, requestFailed
	}
	if !v.isPrivate(permissions, "Resource", resource.ID) {
		return nil, unowned
	}
	secret, err := v.client.GetSecret(ctx, resource.ID)
	if err != nil {
		return nil, requestFailed
	}
	if secret.UserID != v.userID || secret.ResourceID != resource.ID {
		return nil, unowned
	}
	return v.decryptKey(secret.Data, revision)
}

func (v vault) name(ctx context.Context, resource *api.Resource, resourceType *api.ResourceType) (string, error) {
	if resource.Metadata == "" {
		return resource.Name, nil
	}
	text, err := helper.GetResourceMetadata(ctx, v.client, resource, resourceType)
	if err != nil {
		return "", err
	}
	var metadata struct {
		Name string `json:"name"`
	}
	err = json.Unmarshal([]byte(text), &metadata)
	return metadata.Name, err
}

func (v vault) decryptKey(ciphertext, revision string) ([]byte, string) {
	identity, err := v.client.GetUserPrivateKeyCopy()
	if err != nil {
		return nil, requestFailed
	}
	defer identity.ClearPrivateParams()
	decrypt, err := v.client.GetPGPHandle().Decryption().DecryptionKey(identity).VerificationKey(identity).New()
	if err != nil {
		return nil, requestFailed
	}
	defer decrypt.ClearPrivateParams()
	message, err := decrypt.Decrypt([]byte(ciphertext), crypto.Armor)
	if err != nil || message.SignatureError() != nil {
		return nil, unowned
	}
	plaintext := message.Bytes()
	defer clear(plaintext)
	var secret struct {
		Password    string `json:"password"`
		Description string `json:"description"`
	}
	if err := json.Unmarshal(plaintext, &secret); err != nil ||
		secret.Description != marker(revision) || !validKey(secret.Password) {
		return nil, unowned
	}
	return []byte(secret.Password), ok
}

func title(revision string) string {
	return "Arca Provider " + revision
}

func marker(revision string) string {
	return "Arca credential revision " + revision
}
