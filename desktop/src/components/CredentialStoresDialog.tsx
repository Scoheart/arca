import { useCallback, useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { checkCredentialStore, deleteCredentialStore, saveCredentialStore } from "../lib/api";
import type { CredentialStoreConfig } from "../lib/types";
import { DialogShell } from "./DialogShell";
import { CheckIcon, EditIcon, KeyIcon, PlusIcon, RefreshIcon, TrashIcon } from "./icons";
import { useToast } from "./Toast";

type ExternalBackend = Exclude<CredentialStoreConfig["backend"], { kind: "keychain" }>;
type StoreKind = ExternalBackend["kind"];
type FieldNames<Backend extends ExternalBackend> = Backend extends unknown ? Exclude<keyof Backend, "kind"> : never;

interface StoreFieldSpec {
  name: FieldNames<ExternalBackend>;
  label: string;
  placeholder?: string;
  help?: string;
  path?: boolean;
  optional?: boolean;
}

const backendForms = {
  "one-password": {
    label: "1Password",
    defaults: { kind: "one-password", command: "", vault_id: "", token_env: "OP_SERVICE_ACCOUNT_TOKEN" },
    fields: [
      { name: "command", label: "command", placeholder: "/opt/homebrew/bin/op", path: true },
      { name: "vault_id", label: "vault" },
      { name: "token_env", label: "tokenEnv", help: "tokenEnvHelp" },
    ],
  },
  bitwarden: {
    label: "Bitwarden / Vaultwarden",
    defaults: {
      kind: "bitwarden", command: "", server_url: "", appdata_dir: "",
      user_id: "", folder_id: "", session_env: "BW_SESSION",
    },
    fields: [
      { name: "command", label: "bwCommand", placeholder: "/opt/homebrew/bin/bw", path: true },
      { name: "server_url", label: "bwServer" },
      { name: "appdata_dir", label: "bwProfile", path: true },
      { name: "user_id", label: "bwAccount" },
      { name: "folder_id", label: "bwFolder" },
      { name: "session_env", label: "bwSession", help: "bwSessionHelp" },
    ],
  },
  keepassxc: {
    label: "KeePassXC",
    defaults: { kind: "keepassxc", command: "", database_path: "", password_env: "ARCA_KEEPASSXC_PASSWORD" },
    fields: [
      {
        name: "command", label: "keepassCommand",
        placeholder: "/Applications/KeePassXC.app/Contents/MacOS/keepassxc-cli", path: true,
      },
      { name: "database_path", label: "keepassDatabase", path: true },
      { name: "key_file", label: "keepassKeyFile", path: true, optional: true },
      { name: "password_env", label: "keepassPasswordEnv", help: "keepassPasswordHelp" },
    ],
  },
  passbolt: {
    label: "Passbolt",
    defaults: {
      kind: "passbolt", command: "", server_url: "", user_id: "",
      private_key_path: "", server_fingerprint: "", password_env: "ARCA_PASSBOLT_PASSPHRASE",
    },
    fields: [
      { name: "command", label: "passboltCommand", path: true },
      { name: "server_url", label: "bwServer" },
      { name: "user_id", label: "bwAccount" },
      { name: "private_key_path", label: "passboltPrivateKey", path: true },
      { name: "server_fingerprint", label: "passboltFingerprint" },
      { name: "folder_id", label: "passboltFolder", optional: true },
      { name: "password_env", label: "passboltPasswordEnv", help: "passboltPasswordHelp" },
    ],
  },
  openbao: {
    label: "OpenBao",
    defaults: { kind: "openbao", server_url: "", mount: "secret", prefix: "arca/providers", token_env: "BAO_TOKEN" },
    fields: [
      { name: "server_url", label: "bwServer" },
      { name: "mount", label: "baoMount" },
      { name: "prefix", label: "baoPrefix" },
      { name: "token_env", label: "baoTokenEnv", help: "tokenEnvHelp" },
    ],
  },
} satisfies {
  [Kind in StoreKind]: {
    label: string;
    defaults: Extract<ExternalBackend, { kind: Kind }>;
    fields: (StoreFieldSpec & { name: FieldNames<Extract<ExternalBackend, { kind: Kind }>> })[];
  };
};
const availableBackends = Object.keys(backendForms) as StoreKind[];

export function CredentialStoresDialog({
  stores,
  onRefresh,
  onClose,
}: {
  stores: CredentialStoreConfig[];
  onRefresh: () => Promise<void>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { show } = useToast();
  const [loading, setLoading] = useState(true);
  const [checking, setChecking] = useState<string | null>(null);
  const [editing, setEditing] = useState<CredentialStoreConfig | null | undefined>(undefined);
  const [deleting, setDeleting] = useState<CredentialStoreConfig | null>(null);
  const [removing, setRemoving] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      await onRefresh();
    } catch (error) {
      show({ kind: "error", msg: t("credentialStores.loadFailed"), details: error });
    } finally {
      setLoading(false);
    }
  }, [onRefresh, show, t]);

  useEffect(() => { void refresh(); }, [refresh]);

  const check = async (store: CredentialStoreConfig) => {
    setChecking(store.id);
    try {
      await checkCredentialStore(store.id);
      show({ kind: "success", msg: t("credentialStores.connected", { name: store.name }) });
    } catch (error) {
      show({ kind: "error", msg: t("credentialStores.checkFailed", { name: store.name }), details: error });
    } finally {
      setChecking(null);
    }
  };

  const remove = async (store: CredentialStoreConfig) => {
    setRemoving(true);
    try {
      await deleteCredentialStore(store.id);
      setDeleting(null);
      show({ kind: "success", msg: t("credentialStores.removed", { name: store.name }) });
      void refresh();
    } catch (error) {
      show({ kind: "error", msg: t("credentialStores.removeFailed", { name: store.name }), details: error });
    } finally {
      setRemoving(false);
    }
  };

  return (
    <>
      <DialogShell
        kind="detail"
        title={t("credentialStores.title")}
        subtitle={t("credentialStores.description")}
        leading={<span className="arca-dialog-shell-glyph"><KeyIcon className="w-4 h-4" /></span>}
        busy={checking !== null}
        onClose={onClose}
        footerStart={(
          <button type="button" className="btn-ghost" onClick={() => void refresh()} disabled={loading || checking !== null}
            aria-label={t("credentialStores.refresh")} title={t("credentialStores.refresh")}>
            <RefreshIcon className="w-4 h-4" />
          </button>
        )}
        footerEnd={(
          <button type="button" className="btn-primary" onClick={() => setEditing(null)} disabled={loading || checking !== null}>
            <PlusIcon className="w-4 h-4" />
            {t("credentialStores.addConnection")}
          </button>
        )}
      >
        {stores.length ? (
          <ul className="divide-y divide-[var(--border-hairline)] rounded-lg border border-[var(--border-hairline)]">
            {stores.map((store) => (
              <li key={store.id} className="flex items-center gap-2 px-3 py-2">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-semibold" title={store.name}>{store.name}</div>
                  <div className="text-xs text-[var(--text-secondary)]">
                    {store.backend.kind === "keychain" ? t("credentialStores.builtIn") : backendForms[store.backend.kind].label}
                  </div>
                </div>
                <button type="button" className="btn-secondary shrink-0" disabled={loading || checking !== null}
                  aria-label={t("credentialStores.checkNamed", { name: store.name })} onClick={() => void check(store)}>
                  <CheckIcon className="w-4 h-4" />
                  {t(checking === store.id ? "credentialStores.checking" : "credentialStores.check")}
                </button>
                {store.backend.kind !== "keychain" && (
                  <>
                    <button type="button" className="btn-ghost" disabled={loading || checking !== null}
                      title={t("credentialStores.editNamed", { name: store.name })}
                      aria-label={t("credentialStores.editNamed", { name: store.name })} onClick={() => setEditing(store)}>
                      <EditIcon className="w-4 h-4" />
                    </button>
                    <button type="button" className="btn-ghost" disabled={loading || checking !== null}
                      title={t("credentialStores.removeNamed", { name: store.name })}
                      aria-label={t("credentialStores.removeNamed", { name: store.name })} onClick={() => setDeleting(store)}>
                      <TrashIcon className="w-4 h-4" />
                    </button>
                  </>
                )}
              </li>
            ))}
          </ul>
        ) : loading && <p className="text-sm text-[var(--text-secondary)]">{t("common.loading")}</p>}
      </DialogShell>
      {editing !== undefined && (
        <CredentialStoreEditor
          initial={editing}
          onClose={() => setEditing(undefined)}
          onSave={async (store) => {
            await saveCredentialStore(store);
            setEditing(undefined);
            show({ kind: "success", msg: t("credentialStores.saved", { name: store.name }) });
            void refresh();
          }}
        />
      )}
      {deleting && (
        <DialogShell
          kind="review"
          title={t("credentialStores.removeNamed", { name: deleting.name })}
          busy={removing}
          onClose={() => setDeleting(null)}
          footerEnd={(
            <>
              <button type="button" className="btn-secondary" disabled={removing} onClick={() => setDeleting(null)}>
                {t("common.cancel")}
              </button>
              <button type="button" className="btn-danger-solid" disabled={removing} onClick={() => void remove(deleting)}>
                {t("common.delete")}
              </button>
            </>
          )}
        >
          <p className="text-sm text-[var(--text-secondary)]">{t("credentialStores.removeDescription", { name: deleting.name })}</p>
        </DialogShell>
      )}
    </>
  );
}

function CredentialStoreEditor({
  initial,
  onSave,
  onClose,
}: {
  initial: CredentialStoreConfig | null;
  onSave: (store: CredentialStoreConfig) => Promise<void>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const existing = initial?.backend.kind === "keychain" ? undefined : initial?.backend;
  const [name, setName] = useState(initial?.name ?? "");
  const [backend, setBackend] = useState<ExternalBackend>(() => existing ?? backendForms["one-password"].defaults);
  const [busy, setBusy] = useState(false);
  const fields: readonly StoreFieldSpec[] = backendForms[backend.kind].fields;
  const values: Partial<Record<StoreFieldSpec["name"], string>> = backend;
  const complete = Boolean(name.trim() && fields.every(({ name, optional }) => optional || values[name]?.trim()));
  const readOnly = Boolean(existing);

  const save = async () => {
    if (!complete || busy) return;
    const invalidPath = fields.some(({ name, path }) => {
      const value = values[name]?.trim();
      return path && value && !value.startsWith("/");
    });
    if (!existing && invalidPath) {
      toast.show({ kind: "error", msg: t("credentialStores.absolutePathRequired") });
      return;
    }
    const normalized = fields.reduce<ExternalBackend>((draft, field) => ({
      ...draft, [field.name]: values[field.name]?.trim() || undefined,
    }), backend);
    setBusy(true);
    try {
      await onSave({
        id: initial?.id ?? `${backend.kind}-${crypto.randomUUID()}`,
        name: name.trim(),
        backend: existing ?? normalized,
      });
    } catch (error) {
      toast.show({ kind: "error", msg: t("credentialStores.saveFailed"), details: error });
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogShell
      kind="editor"
      size="md"
      title={initial ? t("credentialStores.editNamed", { name: initial.name }) : t("credentialStores.addConnection")}
      busy={busy}
      onClose={onClose}
      footerEnd={(
        <>
          <button type="button" className="btn-secondary" disabled={busy} onClick={onClose}>{t("common.cancel")}</button>
          <button type="button" className="btn-primary" disabled={!complete || busy} onClick={() => void save()}>
            {t(busy ? "common.saving" : "common.save")}
          </button>
        </>
      )}
    >
      <div className="arca-model-form">
        <label>
          <span>{t("credentialStores.name")}</span>
          <input autoFocus className="arca-model-field" value={name} onChange={(event) => setName(event.currentTarget.value)} />
        </label>
        <label>
          <span>{t("credentialStores.type")}</span>
          <select className="arca-model-field" value={backend.kind} disabled={readOnly}
            onChange={(event) => {
              const kind = availableBackends.find((option) => option === event.currentTarget.value);
              if (kind) setBackend(backendForms[kind].defaults);
            }}>
            {availableBackends.map((kind) => (
              <option key={kind} value={kind}>{backendForms[kind].label}</option>
            ))}
          </select>
        </label>
        {fields.map((field) => (
          <StoreField key={field.name} field={field} value={values[field.name] ?? ""} readOnly={readOnly}
            onChange={(value) => setBackend({ ...backend, [field.name]: value })} />
        ))}
      </div>
    </DialogShell>
  );
}


function StoreField({ field, value, readOnly, onChange }: {
  field: StoreFieldSpec;
  value: string;
  readOnly: boolean;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  const helpId = useId();
  return (
    <>
      <label>
        <span>{t(`credentialStores.${field.label}`)}</span>
        <input className="arca-model-field" value={value} readOnly={readOnly} placeholder={field.placeholder}
          aria-describedby={field.help ? helpId : undefined}
          autoComplete="off" autoCapitalize="off" autoCorrect="off" spellCheck={false}
          onChange={(event) => onChange(event.currentTarget.value)} />
      </label>
      {field.help && <p id={helpId} className="-mt-2 text-xs text-[var(--text-secondary)]">{t(`credentialStores.${field.help}`)}</p>}
    </>
  );
}
