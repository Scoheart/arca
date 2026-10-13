import { ResourceIcon, RESOURCE_PRESENTATION } from "./resourcePresentation";
import {
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { addAgent } from "../lib/api";
import type { AgentDefinitionInput } from "../lib/types";
import { formatError } from "../lib/format";
import { AgentGlyph } from "./brandIcons";
import { DialogShell } from "./DialogShell";
import { DialogDisclosure } from "./DialogDisclosure";
import { useToast } from "./Toast";

const FORMATS = [
  { value: "json", label: "JSON" },
  { value: "toml", label: "TOML" },
  { value: "yaml", label: "YAML" },
] as const;

const CATEGORIES = [
  { value: "cli", labelKey: "agents.categoryCli" },
  { value: "desktop", labelKey: "agents.categoryDesktop" },
  { value: "ide", labelKey: "agents.categoryIde" },
  { value: "plugin", labelKey: "agents.categoryPlugin" },
] as const;

const AGENT_ID_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
type CapabilityTab = "mcp" | "skills";
type CapabilityState = "optional" | "incomplete" | "configured";

function isSkillsDirectory(value: string) {
  if (!value.startsWith("~/") || !value.endsWith("/skills")) return false;
  const parts = value.slice(2).split("/");
  return !parts.some((part) => !part || part === "." || part === "..")
    && parts[0] !== ".arca";
}

function isHttpUrl(value: string) {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

/** Create an Agent identity first, then declare the resource writers Arca may
 * manage for it. A completed location includes that writer; an empty location
 * omits it. Custom Model writers require a dedicated safe adapter and are not
 * represented as a generic path-only capability. */
export function AddAgentDialog({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  onAdded: () => Promise<unknown> | void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [id, setId] = useState("");
  const [category, setCategory] = useState("cli");
  const [activeCapability, setActiveCapability] = useState<CapabilityTab>("mcp");
  const [format, setFormat] = useState<"json" | "toml" | "yaml">("json");
  const [key, setKey] = useState("mcpServers");
  const [global, setGlobal] = useState("");
  const [skillsDir, setSkillsDir] = useState("");
  const [skillsDocs, setSkillsDocs] = useState("");
  const [busy, setBusy] = useState(false);
  const mcpTabRef = useRef<HTMLButtonElement>(null);
  const skillsTabRef = useRef<HTMLButtonElement>(null);
  const toast = useToast();

  const trimmedName = name.trim();
  const trimmedId = id.trim();
  const trimmedGlobal = global.trim();
  const trimmedKey = key.trim();
  const trimmedSkillsDir = skillsDir.trim();
  const trimmedSkillsDocs = skillsDocs.trim();
  const skillsTargetId = `${trimmedId}-skills`;
  const mcpStarted = trimmedGlobal.length > 0;
  const skillsStarted = trimmedSkillsDir.length > 0 || trimmedSkillsDocs.length > 0;
  const idValid = AGENT_ID_PATTERN.test(trimmedId)
    && trimmedId.length <= (skillsStarted ? 57 : 64);
  const skillsDirValid = isSkillsDirectory(trimmedSkillsDir);
  const skillsDocsValid = isHttpUrl(trimmedSkillsDocs);
  const mcpConfigured = mcpStarted && trimmedKey.length > 0;
  const skillsConfigured = skillsDirValid && skillsDocsValid;
  const mcpState: CapabilityState = mcpConfigured
    ? "configured"
    : mcpStarted ? "incomplete" : "optional";
  const skillsState: CapabilityState = skillsConfigured
    ? "configured"
    : skillsStarted ? "incomplete" : "optional";
  const incompleteCapabilities = [
    mcpState === "incomplete" ? "MCP" : null,
    skillsState === "incomplete" ? "Skills" : null,
  ].filter(Boolean).join(" + ");
  const hasCapability = mcpConfigured || skillsConfigured;
  const canSubmit = trimmedName.length > 0
    && idValid
    && hasCapability
    && !incompleteCapabilities
    && !busy;
  const selectedCapabilities = [
    mcpConfigured ? "MCP" : null,
    skillsConfigured ? "Skills" : null,
  ].filter(Boolean).join(" + ");
  const displayName = trimmedName || trimmedId || t("agents.newAgent");
  const handleCapabilityTabKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    let nextTab: CapabilityTab | null = null;
    if (event.key === "ArrowRight" || event.key === "End") nextTab = "skills";
    if (event.key === "ArrowLeft" || event.key === "Home") nextTab = "mcp";
    if (!nextTab) return;
    event.preventDefault();
    setActiveCapability(nextTab);
    (nextTab === "mcp" ? mcpTabRef : skillsTabRef).current?.focus();
  };

  const submit = async () => {
    if (!canSubmit) return;
    setBusy(true);
    const definition: AgentDefinitionInput = {
      global: mcpConfigured ? trimmedGlobal : null,
      project: null,
      format: mcpConfigured ? format : "",
      key: mcpConfigured ? trimmedKey : "",
      enabled: true,
      builtin: false,
      name: trimmedName,
      category,
      evidence: "custom",
      verified_at: null,
      docs: skillsConfigured ? trimmedSkillsDocs : null,
      skills: skillsConfigured ? {
        target_id: skillsTargetId,
        global_dir: trimmedSkillsDir,
        aliases: [],
        docs: trimmedSkillsDocs,
        evidence: "official",
        verified_at: new Date().toISOString().slice(0, 10),
        probes: [{ kind: "path", path: trimmedSkillsDir }],
      } : null,
    };
    try {
      await addAgent(trimmedId, definition);
      toast.show({
        kind: "success",
        msg: t("agents.addedToast", { name: trimmedName }),
      });
      await onAdded();
      onClose();
    } catch (error) {
      toast.show({
        kind: "error",
        msg: t("agents.addFailed", { error: formatError(error) }),
      });
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogShell
      className="arca-dialog-agent-editor"
      kind="editor"
      size="md"
      title={t("agents.addTitle")}
      busy={busy}
      onClose={onClose}
      footerStart={(
        <span
          className="arca-agent-create-summary"
          data-state={incompleteCapabilities
            ? "incomplete"
            : hasCapability ? "ready" : "empty"}
          aria-live="polite"
        >
          <span />
          {incompleteCapabilities
            ? t("agents.capabilityIncompleteSummary", {
                capabilities: incompleteCapabilities,
              })
            : hasCapability
            ? t("agents.capabilitySummary", { capabilities: selectedCapabilities })
            : t("agents.capabilityRequired")}
        </span>
      )}
      footerEnd={(
        <>
          <button type="button" onClick={onClose} disabled={busy} className="btn-ghost">
            {t("common.cancel")}
          </button>
          <button
            type="button"
            disabled={!canSubmit}
            onClick={() => void submit()}
            className="btn-primary"
          >
            {busy ? t("agents.adding") : t("agents.addAction")}
          </button>
        </>
      )}
    >
      <div className="arca-agent-create">
        <section className="arca-agent-create-section" aria-labelledby="agent-identity-title">
          <div className="arca-agent-create-section-head">
            <h3 id="agent-identity-title">{t("agents.identityTitle")}</h3>
          </div>

          <div className="arca-agent-identity-card">
            <div className="arca-agent-identity-preview" aria-label={t("agents.identityPreview")}>
              <AgentGlyph id={trimmedId || "new-agent"} name={displayName} size={42} />
            </div>

            <div className="arca-agent-identity-fields">
              <label className="arca-agent-create-field">
                <span>{t("agents.nameLabel")} <i>*</i></span>
                <input
                  autoFocus
                  className="arca-model-field"
                  value={name}
                  placeholder={t("agents.namePlaceholder")}
                  onChange={(event) => setName(event.target.value)}
                />
              </label>
              <label className="arca-agent-create-field">
                <span>{t("agents.idLabel")} <i>*</i></span>
                <input
                  className="arca-model-field"
                  value={id}
                  spellCheck={false}
                  aria-invalid={id.length > 0 && !idValid}
                  placeholder={t("agents.idPlaceholder")}
                  onChange={(event) => setId(event.target.value)}
                />
                {id.length > 0 && !idValid && (
                  <small data-error="true">
                    {t(skillsStarted ? "agents.idInvalidWithSkills" : "agents.idInvalid")}
                  </small>
                )}
              </label>
              <DialogDisclosure title={t("common.advancedSettings")}>
              <label className="arca-agent-create-field" data-wide>
                <span>{t("agents.categoryLabel")}</span>
                <select
                  className="arca-model-field"
                  value={category}
                  onChange={(event) => setCategory(event.target.value)}
                >
                  {CATEGORIES.map((option) => (
                    <option key={option.value} value={option.value}>
                      {t(option.labelKey)}
                    </option>
                  ))}
                </select>
              </label>
              </DialogDisclosure>
            </div>
          </div>
        </section>

        <section className="arca-agent-create-section" aria-labelledby="agent-capabilities-title">
          <div className="arca-agent-create-section-head">
            <h3 id="agent-capabilities-title">{t("agents.capabilitiesTitle")}</h3>
          </div>

          <div
            className="arca-agent-capability-tabs"
            role="tablist"
            aria-label={t("agents.capabilitiesTitle")}
            onKeyDown={handleCapabilityTabKeyDown}
          >
            <button
              ref={mcpTabRef}
              type="button"
              role="tab"
              id="agent-capability-tab-mcp"
              aria-selected={activeCapability === "mcp"}
              aria-controls="agent-capability-panel-mcp"
              tabIndex={activeCapability === "mcp" ? 0 : -1}
              className="arca-agent-capability-tab"
              data-active={activeCapability === "mcp" || undefined}
              data-state={mcpState}
              onClick={() => setActiveCapability("mcp")}
            >
              <span className="arca-agent-capability-tab-icon">
                <ResourceIcon domain="mcp" />
              </span>
              <span className="arca-agent-capability-tab-copy">
                <strong>{RESOURCE_PRESENTATION.mcp.label}</strong>
                <small>{t(`agents.capabilityState.${mcpState}`)}</small>
              </span>
              <i aria-hidden="true" />
            </button>
            <button
              ref={skillsTabRef}
              type="button"
              role="tab"
              id="agent-capability-tab-skills"
              aria-selected={activeCapability === "skills"}
              aria-controls="agent-capability-panel-skills"
              tabIndex={activeCapability === "skills" ? 0 : -1}
              className="arca-agent-capability-tab"
              data-active={activeCapability === "skills" || undefined}
              data-state={skillsState}
              onClick={() => setActiveCapability("skills")}
            >
              <span className="arca-agent-capability-tab-icon">
                <ResourceIcon domain="skill" />
              </span>
              <span className="arca-agent-capability-tab-copy">
                <strong>{RESOURCE_PRESENTATION.skill.label}</strong>
                <small>{t(`agents.capabilityState.${skillsState}`)}</small>
              </span>
              <i aria-hidden="true" />
            </button>
          </div>

          {activeCapability === "mcp" ? (
            <section
              id="agent-capability-panel-mcp"
              role="tabpanel"
              aria-labelledby="agent-capability-tab-mcp"
              className="arca-agent-capability-detail"
            >
              <div className="arca-agent-detail-fields">
                <label className="arca-agent-create-field" data-wide>
                  <span>{t("agents.mcpPathLabel")} <i>*</i></span>
                  <input
                    className="arca-model-field"
                    value={global}
                    spellCheck={false}
                    placeholder={trimmedId
                      ? `~/.${trimmedId}/mcp.json`
                      : t("agents.mcpPathPlaceholder")}
                    onChange={(event) => setGlobal(event.target.value)}
                  />
                </label>
                <DialogDisclosure title={t("common.advancedSettings")} summary={`${format.toUpperCase()} · ${key}`}>
                <div className="arca-agent-create-field">
                  <span>{t("agents.formatLabel")}</span>
                  <div className="arca-agent-format-picker" role="group" aria-label={t("agents.formatLabel")}>
                    {FORMATS.map((candidate) => (
                      <button
                        type="button"
                        key={candidate.value}
                        aria-pressed={format === candidate.value}
                        onClick={() => setFormat(candidate.value)}
                      >
                        {candidate.label}
                      </button>
                    ))}
                  </div>
                </div>
                <label className="arca-agent-create-field">
                  <span>{t("agents.mcpKeyLabel")} <i>*</i></span>
                  <input
                    className="arca-model-field"
                    value={key}
                    spellCheck={false}
                    placeholder="mcpServers"
                    onChange={(event) => setKey(event.target.value)}
                  />
                </label>
                </DialogDisclosure>
              </div>
            </section>
          ) : (
            <section
              id="agent-capability-panel-skills"
              role="tabpanel"
              aria-labelledby="agent-capability-tab-skills"
              className="arca-agent-capability-detail"
            >
              <div className="arca-agent-detail-fields">
                <label className="arca-agent-create-field">
                  <span>{t("agents.skillsPathLabel")} <i>*</i></span>
                  <input
                    className="arca-model-field"
                    value={skillsDir}
                    spellCheck={false}
                    aria-invalid={skillsDir.length > 0 && !skillsDirValid}
                    placeholder={trimmedId
                      ? `~/.${trimmedId}/skills`
                      : t("agents.skillsPathPlaceholder")}
                    onChange={(event) => setSkillsDir(event.target.value)}
                  />
                  {skillsDir.length > 0 && !skillsDirValid && (
                    <small data-error="true">{t("agents.skillsPathInvalid")}</small>
                  )}
                </label>
                <label className="arca-agent-create-field">
                  <span>{t("agents.skillsDocsLabel")} <i>*</i></span>
                  <input
                    className="arca-model-field"
                    value={skillsDocs}
                    type="url"
                    spellCheck={false}
                    aria-invalid={skillsDocs.length > 0 && !skillsDocsValid}
                    placeholder="https://docs.example.com/skills"
                    onChange={(event) => setSkillsDocs(event.target.value)}
                  />
                  {skillsDocs.length > 0 && !skillsDocsValid && (
                    <small data-error="true">{t("agents.skillsDocsInvalid")}</small>
                  )}
                </label>
              </div>
            </section>
          )}
        </section>
      </div>
    </DialogShell>
  );
}
