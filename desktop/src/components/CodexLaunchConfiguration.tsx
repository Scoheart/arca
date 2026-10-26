import { useTranslation } from "react-i18next";
import { codexProfileArguments, type CodexConfigurationSource } from "../lib/codexConfiguration";
import { FormSelect } from "./FormSelect";
import "./CodexConfigurationEditor.css";

export function CodexLaunchConfiguration({ args, disabled, onChange, onEdit }: {
  args: string; disabled: boolean; onChange(args: string): void; onEdit(source: CodexConfigurationSource): void;
}) {
  const { t } = useTranslation();
  const { profile, remaining } = codexProfileArguments(args);
  const source = profile === null ? "native" : profile === "arca" ? "arca" : "custom";
  return <section className="arca-codex-launch-config">
    <span>{t("agentConfiguration.codex.source")}</span>
    <div><FormSelect ariaLabel={t("agentConfiguration.codex.source")} value={source} disabled={disabled}
      options={[{ value: "native", label: t("agentConfiguration.codex.native") }, { value: "arca", label: t("agentConfiguration.codex.arca") },
        ...(source === "custom" ? [{ value: "custom", label: t("agentConfiguration.codex.customProfile", { name: profile }) }] : [])]}
      onChange={value => onChange((value === "arca" ? ["--profile", "arca", ...remaining] : remaining).join("\n"))} />
      <button type="button" className="btn-secondary" disabled={disabled || source === "custom"} onClick={() => onEdit(source === "arca" ? "arca" : "native")}>{t("common.edit")}</button>
    </div>
  </section>;
}
