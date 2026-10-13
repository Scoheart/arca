import type { ConsumptionStatus as Status } from "../lib/types";
import { useTranslation } from "react-i18next";

export function ConsumptionStatus({
  status,
  reason,
  compact = false,
}: {
  status: Status;
  reason?: string | null;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const labels: Record<Status, string> = {
    synced: t("observations.statuses.synced"),
    "external-added": t("observations.statuses.externalAdded"),
    "external-changed": t("observations.statuses.externalChanged"),
    "external-removed": t("observations.statuses.externalRemoved"),
    unparseable: t("observations.statuses.unparseable"),
    ambiguous: t("observations.statuses.ambiguous"),
    unsupported: t("observations.statuses.unsupported"),
  };
  return (
    <span
      className="arca-consumption-status"
      data-status={status}
      data-compact={compact ? "true" : undefined}
      title={reason ?? labels[status]}
      aria-label={compact ? labels[status] : undefined}
    >
      <span aria-hidden="true" />
      {!compact && labels[status]}
    </span>
  );
}
