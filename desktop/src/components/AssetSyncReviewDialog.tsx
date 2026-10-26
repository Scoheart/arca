import { ToastNotice } from "./Toast";
import { ResourceIcon } from "./resourcePresentation";
import { useTranslation } from "react-i18next";
import type { AssetOperationPlan } from "../lib/types";
import { AgentGlyph } from "./brandIcons";
import { DialogDisclosure } from "./DialogDisclosure";
import { DialogShell } from "./DialogShell";
import { RefreshIcon } from "./icons";
import "./AssetSyncReviewDialog.css";

// These observations are the reason for the requested restore, not a second
// problem to solve. The overwrite explanation covers them; retain raw evidence
// in Details. Other warnings stay visible and can_commit remains authoritative.
const RESTORE_OBSERVATIONS = new Set([
  "mcp_config_drift", "mcp_enabled_state_drift", "mcp_target_missing",
]);

export function AssetSyncReviewDialog({
  plan, assets, agents, busy, error, cancelLabel, onCommit, onCancel,
}: {
  plan: AssetOperationPlan;
  assets: Array<{ key: string; name: string }>;
  agents: Array<{ id: string; name: string }>;
  busy: boolean;
  error: string | null;
  cancelLabel: string;
  onCommit(): Promise<unknown> | unknown;
  onCancel(): Promise<unknown> | unknown;
}) {
  const { t } = useTranslation();
  const warnings = [...new Set(plan.warnings.flatMap((warning) => {
    const reason = warning.slice(warning.lastIndexOf(":") + 1).trim();
    if (RESTORE_OBSERVATIONS.has(reason)) return [];
    if (reason === "model_credential_export_plaintext") return [t("syncReview.plaintext")];
    return [/^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$/.test(reason)
      ? t("syncReview.unknownWarning") : warning];
  }))];
  const paths = [...new Set(plan.target_files)];
  return <DialogShell kind="review" size="sm" title={t("syncReview.title")}
    className="arca-sync-dialog" busy={busy} onClose={() => void onCancel()}
    leading={<span className="arca-dialog-shell-glyph"><RefreshIcon className="w-4 h-4" aria-hidden="true" /></span>}
    status={(!plan.can_commit || error) ? <ToastNotice message={error || t("syncReview.blocked")} /> : undefined}
    footerEnd={<>
      <button type="button" className="btn-secondary" disabled={busy} onClick={() => void onCancel()}>{cancelLabel}</button>
      <button type="button" className="btn-primary" disabled={busy || !plan.can_commit} onClick={() => void onCommit()}>
        {busy ? t("syncReview.busy") : t("syncReview.confirm")}
      </button>
    </>}>
    <div className="arca-sync-content">
      <ul className="arca-sync-assets">
        {assets.map((asset) => <li key={asset.key}>
          <span className="arca-sync-asset-icon" aria-hidden="true"><ResourceIcon domain="mcp" /></span>
          <strong>{asset.name}</strong>
        </li>)}
      </ul>
      {agents.length > 0 && <div className="arca-sync-destinations" aria-label={t("syncReview.destination")}>
        <span>{t("syncReview.destination")}</span>
        <ul>{agents.map((agent) => <li key={agent.id}>
          <AgentGlyph id={agent.id} name={agent.name} size={22} /><span>{agent.name}</span>
        </li>)}</ul>
      </div>}
      <p className="arca-sync-description">{t("syncReview.description")}</p>
      {warnings.length > 0 && <div role="note" className="arca-sync-warnings">
        {warnings.map((warning) => <p key={warning}>{warning}</p>)}
      </div>}
      {(paths.length > 0 || plan.warnings.length > 0 || plan.central_changes.some((change) => change.summary.length > 0)) &&
        <DialogDisclosure title={t("syncReview.details")}
          summary={paths.length > 0 ? t("syncReview.files", { count: paths.length }) : undefined}>
          <div className="arca-sync-details">
            {plan.central_changes.map((change, index) => <ul key={index}>
              {change.summary.map((line, lineIndex) => <li key={lineIndex}>{line}</li>)}
            </ul>)}
            {paths.length > 0 && <ul>{paths.map((path) => <li key={path}><code>{path}</code></li>)}</ul>}
            {plan.warnings.length > 0 && <ul>{plan.warnings.map((warning, index) => <li key={index}><code>{warning}</code></li>)}</ul>}
          </div>
        </DialogDisclosure>}
    </div>
  </DialogShell>;
}
