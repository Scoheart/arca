import { useEffect, useRef, useState, type ReactNode } from "react";
import protocolDefinitions from "../../../data/model-protocols.json";
import * as api from "../lib/api";
import { formatError } from "../lib/format";
import type {
  AgentStartupAgentItemView,
  ConsumptionView,
  HookDefinition,
  ModelAdoptionCandidate,
  ModelProfileView,
  ModelProtocol,
  SkillContentKind,
  SkillDetail,
  SkillInventoryItem,
  StartupAgentDefinition,
  SubagentDefinition,
} from "../lib/types";
import { DialogShell } from "./DialogShell";
import { skillRiskLabel, skillSourceText } from "./SkillCard";
import { skillBody } from "./SkillInspector";
import { ProviderGlyph } from "./providerIcons";
import { useToast } from "./Toast";
import { BrainIcon, CheckIcon, CopyIcon, ExternalLinkIcon, HookMarkIcon } from "./icons";
import { Avatar, Badge } from "./ui";
import "./NativePluginsPanel.css";

export type AgentAssetPreviewTarget =
  | {
      kind: "model";
      agentName: string;
      consumption: ConsumptionView;
      profile?: ModelProfileView;
      externalCandidate?: ModelAdoptionCandidate;
    }
  | {
      kind: "skill";
      agentName: string;
      consumption: ConsumptionView;
      centralSkill?: SkillInventoryItem;
      externalSkill?: SkillInventoryItem;
    }
  | {
      kind: "subagent";
      agentName: string;
      consumption: ConsumptionView;
      subagent?: SubagentDefinition;
    }
  | {
      kind: "hook";
      agentName: string;
      consumption: ConsumptionView;
      hook?: HookDefinition;
      centralExists: boolean;
    }
  | {
      kind: "startup_agent";
      agentName: string;
      item: AgentStartupAgentItemView;
      definition?: StartupAgentDefinition;
      cliFlag?: string;
    };

interface PreviewFact {
  key: string;
  label: string;
  value: ReactNode;
  wide?: boolean;
}

interface PreviewContent {
  size: "md" | "wide";
  title: string;
  subtitle?: ReactNode;
  leading: ReactNode;
  body: ReactNode;
  canOpenDefinition: boolean;
}

function protocolLabel(protocol: ModelProtocol): string {
  return protocolDefinitions.find((item) => item.id === protocol)?.label ?? protocol;
}

function consumptionStatusLabel(item: ConsumptionView): string {
  switch (item.status) {
    case "synced":
      return item.enabled === false ? "已停用" : "已同步";
    case "external-added":
      return "外部新增";
    case "external-changed":
      return "配置已变更";
    case "external-removed":
      return "配置已移除";
    case "unparseable":
      return "配置无法解析";
    case "ambiguous":
      return "存在配置冲突";
    case "unsupported":
      return "不兼容";
  }
}

function consumptionStatusTone(item: ConsumptionView): "neutral" | "success" | "warning" | "info" {
  if (item.status === "synced") return item.enabled === false ? "neutral" : "success";
  if (item.status === "external-added" || item.status === "external-changed") return "warning";
  return "neutral";
}

const SKILL_CONTENT_KIND_LABELS: Record<SkillContentKind, string> = {
  instructions: "指令规范",
  automation: "含可执行脚本",
  reference: "参考文档",
  assets: "资源与模板",
};

function startupSourceLabel(source: AgentStartupAgentItemView["source"]): string {
  if (source === "builtin") return "内置";
  if (source === "central") return "中央库";
  return "本地发现";
}

function credentialKindLabel(kind: ModelAdoptionCandidate["credential_kind"]): string {
  switch (kind) {
    case "environment-reference":
      return "环境变量";
    case "literal":
      return "明文凭据";
    case "external-command":
      return "外部命令";
    case "none":
      return "无凭据";
  }
}

function FactsCard({ facts }: { facts: PreviewFact[] }) {
  if (facts.length === 0) return null;
  return (
    <dl className="arca-native-facts">
      {facts.map((fact) => (
        <div key={fact.key} className={fact.wide ? "arca-native-fact-wide" : undefined}>
          <dt>{fact.label}</dt>
          <dd>{fact.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function CopyableCodeSection({
  heading,
  code,
  copyLabel,
  prose = false,
  onCopy,
}: {
  heading: string;
  code: string;
  copyLabel: string;
  prose?: boolean;
  onCopy: (label: string, value: string) => void;
}) {
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    setCopied(false);
    return () => {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [code]);

  const handleClick = () => {
    onCopy(copyLabel, code);
    setCopied(true);
    if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => {
      setCopied(false);
      timerRef.current = null;
    }, 1500);
  };

  return (
    <section className="arca-preview-card" aria-label={heading}>
      <div className="arca-preview-card-bar">
        <span className="arca-preview-card-title">{heading}</span>
        <button type="button" className="arca-preview-copy-btn" onClick={handleClick}>
          {copied ? <CheckIcon className="w-3 h-3" /> : <CopyIcon className="w-3 h-3" />}
          <span>{copied ? "已复制" : "复制"}</span>
        </button>
      </div>
      <pre tabIndex={0} className={prose ? "arca-preview-prose" : undefined}>
        <code>{code}</code>
      </pre>
    </section>
  );
}

function BadgeGroupSection({
  heading,
  items,
  tone = "neutral",
}: {
  heading: string;
  items: string[];
  tone?: "neutral" | "success" | "warning";
}) {
  if (items.length === 0) return null;
  return (
    <section className="arca-native-detail-section">
      <div className="arca-preview-section-label">{heading}</div>
      <div className="arca-preview-badges">
        {items.map((entry) => (
          <Badge key={entry} tone={tone}>
            {entry}
          </Badge>
        ))}
      </div>
    </section>
  );
}

function buildModelPreview(
  target: Extract<AgentAssetPreviewTarget, { kind: "model" }>,
  onCopy: (label: string, value: string) => void,
): PreviewContent {
  const { consumption, profile, externalCandidate, agentName } = target;
  const profileId = consumption.asset.domain === "model" ? consumption.asset.profile_id : "";
  const isExternal = consumption.ownership === "external";
  const title = profile?.name ?? externalCandidate?.name ?? externalCandidate?.model ?? profileId;
  const provider = profile?.provider ?? externalCandidate?.provider ?? "custom";
  const protocol = profile?.protocol ?? externalCandidate?.protocol;
  const isActive = Boolean(consumption.desired_active ? consumption.active !== false : consumption.active);
  const activeState = consumption.desired_active
    ? consumption.active === false
      ? "待切换"
      : "当前使用"
    : consumption.active
      ? "当前使用"
      : "备用";

  const facts: PreviewFact[] = [
    {
      key: "model",
      label: "模型",
      value: <code>{profile?.model ?? externalCandidate?.model ?? profileId}</code>,
    },
    { key: "provider", label: "渠道", value: provider },
  ];
  if (profile) {
    facts.push({
      key: "reasoning",
      label: "思考模式",
      value: profile.reasoning === true ? "开启" : profile.reasoning === false ? "关闭" : "跟随宿主",
    });
  }
  if (profile?.context_window != null) {
    facts.push({
      key: "context",
      label: "上下文窗口",
      value: profile.context_window.toLocaleString(),
    });
  }
  if (profile?.max_output_tokens != null) {
    facts.push({
      key: "max-output",
      label: "最大输出",
      value: profile.max_output_tokens.toLocaleString(),
    });
  }
  const envKey = profile?.env_key ?? externalCandidate?.env_key;
  if (envKey) {
    facts.push({ key: "env-key", label: "环境变量", value: <code>{envKey}</code> });
  }
  if (externalCandidate) {
    facts.push({
      key: "cred",
      label: "凭据",
      value: credentialKindLabel(externalCandidate.credential_kind),
    });
  } else if (profile?.credential_saved) {
    facts.push({ key: "cred", label: "凭据", value: "已保存" });
  }

  const body = (
    <>
      {!profile && !externalCandidate && (
        <p className="arca-native-notice">仅在 {agentName} 中观测到此 Model 引用。</p>
      )}
      {consumption.reason && <p className="arca-native-notice">{consumption.reason}</p>}
      <FactsCard facts={facts} />
      {(profile?.base_url || externalCandidate?.base_url) && (
        <CopyableCodeSection
          heading="Base URL"
          code={profile?.base_url ?? externalCandidate?.base_url ?? ""}
          copyLabel="Base URL"
          onCopy={onCopy}
        />
      )}
    </>
  );

  return {
    size: "md",
    title,
    subtitle: (
      <div className="arca-preview-badges">
        <Badge tone={isActive ? "info" : "neutral"}>{activeState}</Badge>
        {protocol && <Badge tone="neutral">{protocolLabel(protocol)}</Badge>}
        {isExternal && <Badge tone="warning">外部配置</Badge>}
      </div>
    ),
    leading:
      profile || externalCandidate ? (
        <ProviderGlyph id={provider} name={title} size={32} />
      ) : (
        <BrainIcon className="w-5 h-5 text-primary" />
      ),
    body,
    canOpenDefinition: !isExternal && Boolean(profile),
  };
}

function buildSubagentPreview(
  target: Extract<AgentAssetPreviewTarget, { kind: "subagent" }>,
  onCopy: (label: string, value: string) => void,
): PreviewContent {
  const { consumption, subagent } = target;
  const name = consumption.asset.domain === "subagent" ? consumption.asset.name : "";
  const isExternal = consumption.ownership === "external";
  const title = subagent?.display_name || name;
  const identifier = subagent?.name ?? name;

  const facts: PreviewFact[] = [];
  if (identifier && identifier !== title) {
    facts.push({ key: "id", label: "标识", value: <code>{identifier}</code> });
  }
  facts.push({ key: "model", label: "模型", value: subagent?.model || "inherit" });
  if (subagent?.allow_subagents) {
    facts.push({ key: "nested", label: "子代理", value: "允许嵌套" });
  }
  if (consumption.target?.global_dir) {
    facts.push({
      key: "dir",
      label: "目录",
      value: <code>{consumption.target.global_dir}</code>,
      wide: true,
    });
  }

  const body = (
    <>
      {subagent?.description && <p className="arca-preview-summary">{subagent.description}</p>}
      {!subagent && <p className="arca-native-notice">未找到 “{name}” 的中央定义。</p>}
      <FactsCard facts={facts} />
      <BadgeGroupSection heading="允许工具" items={subagent?.tools ?? []} />
      <BadgeGroupSection heading="MCP 依赖" items={subagent?.mcp_dependencies ?? []} tone="success" />
      {subagent?.system_prompt && (
        <CopyableCodeSection
          heading="System Prompt"
          code={subagent.system_prompt}
          copyLabel="System Prompt"
          onCopy={onCopy}
        />
      )}
    </>
  );

  return {
    size: subagent?.system_prompt ? "wide" : "md",
    title,
    subtitle: (
      <div className="arca-preview-badges">
        <Badge tone="neutral">{subagent?.readonly ? "只读沙盒" : "标准读写"}</Badge>
        <Badge tone={consumptionStatusTone(consumption)}>{consumptionStatusLabel(consumption)}</Badge>
      </div>
    ),
    leading: <Avatar seed={name} kind="subagent" size={32} />,
    body,
    canOpenDefinition: !isExternal && Boolean(subagent),
  };
}

function buildHookPreview(
  target: Extract<AgentAssetPreviewTarget, { kind: "hook" }>,
  onCopy: (label: string, value: string) => void,
): PreviewContent {
  const { consumption, hook, centralExists, agentName } = target;
  const name = consumption.asset.domain === "hook" ? consumption.asset.name : "";
  const title = hook?.display_name || name;
  const identifier = hook?.name ?? name;

  const facts: PreviewFact[] = [];
  if (identifier && identifier !== title) {
    facts.push({ key: "id", label: "标识", value: <code>{identifier}</code>, wide: true });
  }
  facts.push({ key: "matcher", label: "Matcher", value: <code>{hook?.matcher || "*"}</code> });
  if (hook) {
    facts.push({ key: "timeout", label: "超时", value: `${hook.timeout} 秒` });
  }

  const body = (
    <>
      {hook?.description && <p className="arca-preview-summary">{hook.description}</p>}
      {!hook && <p className="arca-native-notice">未找到 Hook “{name}” 的详细定义。</p>}
      <FactsCard facts={facts} />
      {hook?.command && (
        <CopyableCodeSection
          heading="执行命令"
          code={hook.command}
          copyLabel="Hook 命令"
          onCopy={onCopy}
        />
      )}
      {hook?.command_windows && (
        <CopyableCodeSection
          heading="Windows 命令"
          code={hook.command_windows}
          copyLabel="Windows Hook 命令"
          onCopy={onCopy}
        />
      )}
    </>
  );

  return {
    size: "md",
    title,
    subtitle: (
      <div className="arca-preview-badges">
        {hook?.event && <Badge tone="info">{hook.event}</Badge>}
        {hook && (
          <Badge tone={hook.enabled ? "success" : "neutral"}>
            {hook.enabled ? "已启用" : "已停用"}
          </Badge>
        )}
        {!centralExists && <Badge tone="neutral">{agentName} 本地</Badge>}
      </div>
    ),
    leading: <HookMarkIcon className="w-5 h-5 text-blue-500" />,
    body,
    canOpenDefinition: centralExists,
  };
}

function buildStartupAgentPreview(
  target: Extract<AgentAssetPreviewTarget, { kind: "startup_agent" }>,
  onCopy: (label: string, value: string) => void,
): PreviewContent {
  const { item, definition, cliFlag } = target;
  const title = definition?.display_name || item.name;
  const description = item.description || definition?.description;
  const cliCommand = `${cliFlag || "--agent"} ${item.name}`;

  const facts: PreviewFact[] = [];
  if (item.id && item.id !== title) {
    facts.push({ key: "id", label: "标识", value: <code>{item.id}</code> });
  }
  facts.push({ key: "model", label: "模型", value: definition?.model || item.model || "默认" });
  if (definition?.mode) {
    facts.push({ key: "mode", label: "模式", value: definition.mode });
  }
  if (item.config_path) {
    facts.push({
      key: "config",
      label: "配置",
      value: <code>{item.config_path}</code>,
      wide: true,
    });
  }

  const body = (
    <>
      {description && <p className="arca-preview-summary">{description}</p>}
      <FactsCard facts={facts} />
      <CopyableCodeSection
        heading="CLI 参数"
        code={cliCommand}
        copyLabel="CLI 参数"
        onCopy={onCopy}
      />
      <BadgeGroupSection heading="允许工具" items={definition?.tools ?? []} />
      {definition?.system_prompt && (
        <CopyableCodeSection
          heading="System Prompt"
          code={definition.system_prompt}
          copyLabel="System Prompt"
          onCopy={onCopy}
        />
      )}
    </>
  );

  return {
    size: definition?.system_prompt ? "wide" : "md",
    title,
    subtitle: (
      <div className="arca-preview-badges">
        <Badge tone={item.is_active ? "info" : "neutral"}>
          {item.is_active ? "当前使用" : item.is_default ? "默认" : startupSourceLabel(item.source)}
        </Badge>
        {item.is_active && <Badge tone="neutral">{startupSourceLabel(item.source)}</Badge>}
        {definition?.readonly && <Badge tone="neutral">只读沙盒</Badge>}
      </div>
    ),
    leading: <Avatar seed={item.name} kind="startup_agent" size={32} />,
    body,
    canOpenDefinition: item.source === "central",
  };
}

function buildSkillPreview({
  target,
  detail,
  loading,
  error,
  onCopy,
}: {
  target: Extract<AgentAssetPreviewTarget, { kind: "skill" }>;
  detail: SkillDetail | null;
  loading: boolean;
  error: string | null;
  onCopy: (label: string, value: string) => void;
}): PreviewContent {
  const { consumption, centralSkill, externalSkill } = target;
  const name = consumption.asset.domain === "skill" ? consumption.asset.name : "";
  const item = centralSkill ?? externalSkill;
  const isExternal = consumption.ownership === "external";
  const localDir = item?.location.kind !== "central"
    ? item?.location.global_dir || consumption.target?.global_dir
    : undefined;
  const bodyText = detail ? skillBody(detail.skill_md) : "";

  const facts: PreviewFact[] = [];
  if (item?.source) {
    facts.push({ key: "source", label: "来源", value: skillSourceText(item.source), wide: true });
  } else if (isExternal) {
    facts.push({ key: "source", label: "来源", value: "本地外部副本" });
  }
  if (item) {
    facts.push({
      key: "kind",
      label: "内容",
      value: `${SKILL_CONTENT_KIND_LABELS[item.content_kind]}${detail ? ` · ${detail.files.length} 个文件` : ""}`,
    });
  }
  if (localDir) {
    facts.push({ key: "dir", label: "位置", value: <code>{localDir}</code>, wide: true });
  }

  const body = (
    <>
      {item?.description && <p className="arca-preview-summary">{item.description}</p>}
      {!item && (
        <p className="arca-native-notice">未加载到 “{name}” 的 Skill 资产清单。</p>
      )}
      {consumption.reason && <p className="arca-native-notice">{consumption.reason}</p>}
      <FactsCard facts={facts} />
      {loading && <p className="arca-native-muted">正在加载正文…</p>}
      {!loading && error && (
        <p className="arca-native-notice" role="alert" style={{ color: "var(--color-red)" }}>
          读取 SKILL.md 失败：{error}
        </p>
      )}
      {!loading && !error && detail && (
        <>
          {detail.skill_md_truncated && (
            <p className="arca-native-notice">正文内容较长，以下展示已截断。</p>
          )}
          <CopyableCodeSection
            heading="SKILL.md"
            code={bodyText || "暂无正文内容。"}
            copyLabel="SKILL.md"
            prose
            onCopy={onCopy}
          />
        </>
      )}
    </>
  );

  return {
    size: "wide",
    title: item?.name || name,
    subtitle: (
      <div className="arca-preview-badges">
        <Badge tone={consumptionStatusTone(consumption)}>{consumptionStatusLabel(consumption)}</Badge>
        {item?.risk && item.risk.finding_count > 0 && (
          <Badge tone="warning">
            {`${skillRiskLabel(item.risk.level)} · ${item.risk.finding_count}`}
          </Badge>
        )}
      </div>
    ),
    leading: <Avatar seed={name} kind="skill" size={32} />,
    body,
    canOpenDefinition: !isExternal && Boolean(centralSkill),
  };
}

export function AgentAssetPreviewDialog({
  target,
  onClose,
  onOpenDefinition,
}: {
  target: AgentAssetPreviewTarget | null;
  onClose: () => void;
  onOpenDefinition?: () => void;
}) {
  const { show: showToast } = useToast();
  const [skillDetail, setSkillDetail] = useState<SkillDetail | null>(null);
  const [skillLoading, setSkillLoading] = useState(false);
  const [skillError, setSkillError] = useState<string | null>(null);

  const skillIdentity =
    target?.kind === "skill" ? (target.centralSkill ?? target.externalSkill)?.identity ?? null : null;

  useEffect(() => {
    if (!skillIdentity) {
      setSkillDetail(null);
      setSkillLoading(false);
      setSkillError(null);
      return;
    }
    let active = true;
    setSkillLoading(true);
    setSkillError(null);
    api
      .getSkillDetail(skillIdentity)
      .then((detail) => {
        if (active) setSkillDetail(detail);
      })
      .catch((err) => {
        if (active) {
          setSkillDetail(null);
          setSkillError(formatError(err));
        }
      })
      .finally(() => {
        if (active) setSkillLoading(false);
      });
    return () => {
      active = false;
    };
  }, [skillIdentity]);

  if (!target) return null;

  const handleCopy = (label: string, value: string) => {
    void api
      .copyToClipboard(value)
      .then(() => {
        showToast({ kind: "success", msg: `已复制 ${label}` });
      })
      .catch((err) => {
        showToast({ kind: "error", msg: `复制失败：${formatError(err)}` });
      });
  };

  let preview: PreviewContent;
  switch (target.kind) {
    case "model":
      preview = buildModelPreview(target, handleCopy);
      break;
    case "skill":
      preview = buildSkillPreview({
        target,
        detail: skillDetail,
        loading: skillLoading,
        error: skillError,
        onCopy: handleCopy,
      });
      break;
    case "subagent":
      preview = buildSubagentPreview(target, handleCopy);
      break;
    case "hook":
      preview = buildHookPreview(target, handleCopy);
      break;
    case "startup_agent":
      preview = buildStartupAgentPreview(target, handleCopy);
      break;
  }

  return (
    <DialogShell
      kind="detail"
      size={preview.size}
      className="arca-native-dialog"
      title={preview.title}
      subtitle={preview.subtitle}
      leading={preview.leading}
      closeLabel="关闭详情"
      onClose={onClose}
      footerEnd={
        <>
          {preview.canOpenDefinition && onOpenDefinition && (
            <button
              type="button"
              className="btn-secondary inline-flex items-center gap-1.5"
              onClick={() => {
                onClose();
                onOpenDefinition();
              }}
            >
              <ExternalLinkIcon className="w-3.5 h-3.5" />
              <span>打开中央定义</span>
            </button>
          )}
          <button type="button" className="btn-ghost" onClick={onClose}>
            关闭
          </button>
        </>
      }
    >
      {preview.body}
    </DialogShell>
  );
}
