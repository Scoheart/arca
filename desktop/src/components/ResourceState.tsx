import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ToastNotice } from "./Toast";

export type ResourceStateKind = "loading" | "empty" | "no-match" | "read-error" | "recovery";

export function ResourceState({
  kind,
  icon,
  title,
  detail,
  action,
}: {
  kind: ResourceStateKind;
  icon?: ReactNode;
  title: string;
  detail?: ReactNode;
  action?: ReactNode;
}) {
  const { t } = useTranslation();
  if (kind === "loading") {
    return (
      <div className="arca-resource-state" data-state-kind={kind} role="status" aria-label={title}>
        <div className="arca-resource-skeleton" aria-hidden="true">
          {Array.from({ length: 6 }, (_, index) => <span key={index} />)}
        </div>
        <span className="sr-only">{title}</span>
      </div>
    );
  }

  const feedback = kind === "read-error" || kind === "recovery";
  return (
    <div className="arca-resource-state" data-state-kind={kind} role="status">
      {feedback && <ToastNotice kind={kind === "recovery" ? "warning" : "error"}
        message={[title, typeof detail === "string" ? detail : null].filter(Boolean).join(" · ")} />}
      {icon != null && <span className="arca-resource-state-icon">{icon}</span>}
      <strong>{feedback ? t("centralAssets.noContent") : title}</strong>
      {!feedback && detail != null && <span className="arca-resource-state-detail">{detail}</span>}
      {action != null && <div className="arca-resource-state-action">{action}</div>}
    </div>
  );
}
