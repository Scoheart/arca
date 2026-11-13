import { useEffect, useState, type ReactNode } from "react";
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
  SkillDetail,
  SkillContentKind,
  SkillInventoryItem,
  StartupAgentDefinition,
  SubagentDefinition,
} from "../lib/types";
import { DialogShell } from "./DialogShell";
import { skillRiskLabel, skillSourceText } from "./SkillCard";
import { skillBody } from "./SkillInspector";
import { ProviderGlyph } from "./providerIcons";
import { useToast } from "./Toast";
import { BrainIcon, CopyIcon, ExternalLinkIcon, HookMarkIcon } from "./icons";
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

interface PreviewContent {
  title: string;
  subtitle: string;
  leading: ReactNode;
  body: ReactNode;
  canOpenDefinition: boolean;
  footerNote: string;
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
      return "当前配置不兼容";
  }
}

const SKILL_CONTENT_KIND_LABELS: Record<SkillContentKind, string> = {
  instructions: "指令规范 (SKILL.md)",
  automation: "包含可执行脚本",
  reference: "参考文档",
  assets: "辅助资源与模板",
};

function startupSourceLabel(source: AgentStartupAgentItemView["source"]): string {
  if (source === "builtin") return "内置";
  if (source === "central") return "中央库";
  return "本地发现";
}

function credentialKindLabel(kind: ModelAdoptionCandidate["credential_kind"]): string {
  switch (kind) {
    case "environment-reference":
      return "环境变量引用";
    case "literal":
      return "明文凭据";
    case "external-command":
      return "外部命令获取";
    case "none":
      return "无凭据";
  }
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function CopyableCodeSection({
  heading,
  code,
  copyLabel,
  onCopy,
}: {
  heading: string;
  code: string;
  copyLabel: string;
  onCopy: (label: string, value: string) => void;
}) {
  return (
    <section className="arca-native-detail-section arca-mcp-configuration">
      <div className="flex items-center justify-between gap-2 mb-2">
        <h3 style={{ margin: 0 }}>{heading}</h3>
        <button
          type="button"
          className="btn-ghost text-xs py-1 px-2 inline-flex items-center gap-1"
          onClick={() => onCopy(copyLabel, code)}
        >
          <CopyIcon className="w-3.5 h-3.5" />
          <span>复制</span>
        </button>
      </div>
      <pre style={{ maxHeight: 320, overflowY: "auto" }}>
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
      <h3>{heading}</h3>
      <div className="flex flex-wrap gap-1.5">
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
  const activeState = consumption.desired_active
    ? consumption.active === false
      ? "期望设为当前"
      : "当前使用"
    : consumption.active
      ? "Agent 实际当前"
      : "已配置（备用）";

  const body = (
    <>
      {!profile && !externalCandidate && (
        <p className="arca-native-notice">
          当前仅在 {agentName} 中观测到此 Model 引用，未能匹配到中央资产定义或完整外部元数据。
        </p>
      )}
      {consumption.reason && <p className="arca-native-notice">同步说明：{consumption.reason}</p>}
      <dl className="arca-native-facts">
        <Fact label="模型标识">
          <code>{profile?.model ?? externalCandidate?.model ?? profileId}</code>
        </Fact>
        <Fact label="接入渠道">{provider}</Fact>
        <Fact label="通信协议">{protocol ? protocolLabel(protocol) : "—"}</Fact>
        <Fact label="使用状态">{activeState}</Fact>
        <Fact label="资产来源">{isExternal ? "本地外部配置" : "中央 Models 资产库"}</Fact>
        <Fact label="同步状态">{consumptionStatusLabel(consumption)}</Fact>
        {profile && (
          <Fact label="思考模式">
            {profile.reasoning === true ? "已开启" : profile.reasoning === false ? "已关闭" : "由宿主决定"}
          </Fact>
        )}
        {profile?.context_window != null && (
          <Fact label="上下文窗口">{profile.context_window.toLocaleString()}</Fact>
        )}
        {profile?.max_output_tokens != null && (
          <Fact label="最大输出 Tokens">{profile.max_output_tokens.toLocaleString()}</Fact>
        )}
        {(profile?.env_key || externalCandidate?.env_key) && (
          <Fact label="环境变量键">
            <code>{profile?.env_key ?? externalCandidate?.env_key}</code>
          </Fact>
        )}
        {externalCandidate && (
          <Fact label="凭据形态">{credentialKindLabel(externalCandidate.credential_kind)}</Fact>
        )}
        {profile && (
          <Fact label="凭据状态">{profile.credential_saved ? "已保存" : "未保存或环境变量提供"}</Fact>
        )}
      </dl>
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
    title,
    subtitle: externalCandidate
      ? "外部 Model 配置 · 未纳入中央模型库"
      : profile
        ? `${protocolLabel(profile.protocol)} · ${profile.provider}`
        : "Model 资产预览",
    leading: profile || externalCandidate ? (
      <ProviderGlyph id={provider} name={title} size={32} />
    ) : (
      <BrainIcon className="w-5 h-5 text-primary" />
    ),
    body,
    canOpenDefinition: !isExternal && Boolean(profile),
    footerNote: isExternal ? `只读预览 · ${agentName} 外部配置` : `只读预览 · ${agentName}`,
  };
}

function buildSubagentPreview(
  target: Extract<AgentAssetPreviewTarget, { kind: "subagent" }>,
  onCopy: (label: string, value: string) => void,
): PreviewContent {
  const { consumption, subagent, agentName } = target;
  const name = consumption.asset.domain === "subagent" ? consumption.asset.name : "";
  const isExternal = consumption.ownership === "external";

  const body = (
    <>
      {!subagent && (
        <p className="arca-native-notice">未找到名为 “{name}” 的中央 Subagent 定义。</p>
      )}
      {subagent?.description && <p className="arca-native-notice">{subagent.description}</p>}
      <dl className="arca-native-facts">
        <Fact label="标识符">
          <code>{subagent?.name ?? name}</code>
        </Fact>
        <Fact label="模型绑定">{subagent?.model || "inherit（继承宿主）"}</Fact>
        <Fact label="权限模式">{subagent?.readonly ? "只读沙盒" : "标准读写"}</Fact>
        <Fact label="嵌套调用">{subagent?.allow_subagents ? "允许" : "禁止"}</Fact>
        <Fact label="同步状态">{consumptionStatusLabel(consumption)}</Fact>
        {consumption.target?.global_dir && (
          <Fact label="生效目录">
            <code>{consumption.target.global_dir}</code>
          </Fact>
        )}
      </dl>
      <BadgeGroupSection heading="允许工具" items={subagent?.tools ?? []} />
      <BadgeGroupSection heading="MCP 依赖" items={subagent?.mcp_dependencies ?? []} tone="success" />
      {subagent?.system_prompt && (
        <CopyableCodeSection
          heading="系统提示词 (System Prompt)"
          code={subagent.system_prompt}
          copyLabel="System Prompt"
          onCopy={onCopy}
        />
      )}
    </>
  );

  return {
    title: subagent?.display_name || name,
    subtitle: subagent ? `${subagent.name} · Subagent 详情` : `${name} · Subagent 资产`,
    leading: <Avatar seed={name} kind="subagent" size={32} />,
    body,
    canOpenDefinition: !isExternal && Boolean(subagent),
    footerNote: `只读预览 · ${agentName}`,
  };
}

function buildHookPreview(
  target: Extract<AgentAssetPreviewTarget, { kind: "hook" }>,
  onCopy: (label: string, value: string) => void,
): PreviewContent {
  const { consumption, hook, centralExists, agentName } = target;
  const name = consumption.asset.domain === "hook" ? consumption.asset.name : "";

  const body = (
    <>
      {!hook && <p className="arca-native-notice">未找到 Hook “{name}” 的详细定义。</p>}
      {hook?.description && <p className="arca-native-notice">{hook.description}</p>}
      <dl className="arca-native-facts">
        <Fact label="标识符">
          <code>{hook?.name ?? name}</code>
        </Fact>
        <Fact label="触发事件">{hook?.event || "—"}</Fact>
        <Fact label="启用状态">{hook ? (hook.enabled ? "已启用" : "已停用") : "—"}</Fact>
        <Fact label="匹配器 (Matcher)">
          <code>{hook?.matcher || "全局 (*)"}</code>
        </Fact>
        <Fact label="超时时间">{hook ? `${hook.timeout} 秒` : "—"}</Fact>
        <Fact label="资产来源">{centralExists ? "中央 Hooks 资产库" : `仅 ${agentName} 本地`}</Fact>
      </dl>
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
          heading="Windows 执行命令"
          code={hook.command_windows}
          copyLabel="Windows Hook 命令"
          onCopy={onCopy}
        />
      )}
    </>
  );

  return {
    title: hook?.display_name || name,
    subtitle: hook ? `${hook.event} · ${hook.enabled ? "已启用" : "已停用"}` : `${name} · Hook 详情`,
    leading: <HookMarkIcon className="w-5 h-5 text-blue-500" />,
    body,
    canOpenDefinition: centralExists,
    footerNote: centralExists ? `只读预览 · ${agentName}` : `只读预览 · ${agentName} 本地 Hook`,
  };
}

function buildStartupAgentPreview(
  target: Extract<AgentAssetPreviewTarget, { kind: "startup_agent" }>,
  onCopy: (label: string, value: string) => void,
): PreviewContent {
  const { item, definition, cliFlag, agentName } = target;
  const description = item.description || definition?.description;
  const cliCommand = `${cliFlag || "--agent"} ${item.name}`;

  const body = (
    <>
      {description && <p className="arca-native-notice">{description}</p>}
      <dl className="arca-native-facts">
        <Fact label="Agent 标识">
          <code>{item.id}</code>
        </Fact>
        <Fact label="来源">{startupSourceLabel(item.source)}</Fact>
        <Fact label="当前状态">
          {item.is_active ? "当前启动身份" : item.is_default ? "默认身份" : "可选身份"}
        </Fact>
        <Fact label="默认模型">{definition?.model || item.model || "跟随宿主配置"}</Fact>
        {definition && (
          <>
            <Fact label="运行模式">{definition.mode || "标准"}</Fact>
            <Fact label="权限模式">{definition.readonly ? "只读沙盒" : "标准读写"}</Fact>
          </>
        )}
        {item.config_path && (
          <Fact label="配置文件">
            <code>{item.config_path}</code>
          </Fact>
        )}
      </dl>
      <CopyableCodeSection
        heading="CLI 启动参数"
        code={cliCommand}
        copyLabel="CLI 参数"
        onCopy={onCopy}
      />
      <BadgeGroupSection heading="允许工具" items={definition?.tools ?? []} />
      {definition?.system_prompt ? (
        <CopyableCodeSection
          heading="系统提示词 (System Prompt)"
          code={definition.system_prompt}
          copyLabel="System Prompt"
          onCopy={onCopy}
        />
      ) : (
        <p className="arca-native-muted" style={{ marginTop: 14 }}>
          {item.source === "central"
            ? "未配置独立的 System Prompt。"
            : "该启动身份由宿主内置或本地配置提供，未包含中央定义正文。"}
        </p>
      )}
    </>
  );

  return {
    title: definition?.display_name || item.name,
    subtitle: `${item.id} · ${startupSourceLabel(item.source)}${item.is_active ? " · 当前使用" : ""}`,
    leading: <Avatar seed={item.name} kind="startup_agent" size={32} />,
    body,
    canOpenDefinition: item.source === "central",
    footerNote: `只读预览 · ${agentName}`,
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
  const { consumption, centralSkill, externalSkill, agentName } = target;
  const name = consumption.asset.domain === "skill" ? consumption.asset.name : "";
  const item = centralSkill ?? externalSkill;
  const isExternal = consumption.ownership === "external";
  const locationLabel = item?.location.kind === "central"
    ? "中央 Skills 资产库"
    : item?.location.global_dir || consumption.target?.global_dir || "Agent 本地目录";
  const bodyText = detail ? skillBody(detail.skill_md) : "";

  const body = (
    <>
      {!item && (
        <p className="arca-native-notice">
          当前未能加载 “{name}” 的 Skill 资产清单，可能已从本地目录或中央仓库移除。
        </p>
      )}
      {item?.description && <p className="arca-native-notice">{item.description}</p>}
      <dl className="arca-native-facts">
        <Fact label="Skill 标识">
          <code>{name}</code>
        </Fact>
        <Fact label="状态">{consumptionStatusLabel(consumption)}</Fact>
        <Fact label="资产来源">{item ? skillSourceText(item.source) : isExternal ? "外部副本 · 来源未知" : "中央资产库"}</Fact>
        <Fact label="存储位置">
          <code>{locationLabel}</code>
        </Fact>
        <Fact label="内容形态">
          {item ? `${SKILL_CONTENT_KIND_LABELS[item.content_kind]}${detail ? ` · ${detail.files.length} 个文件` : ""}` : "—"}
        </Fact>
        {item?.risk && (
          <Fact label="风险检查">
            {item.risk.finding_count > 0
              ? `${skillRiskLabel(item.risk.level)} (${item.risk.finding_count})`
              : skillRiskLabel(item.risk.level)}
          </Fact>
        )}
      </dl>
      {consumption.reason && <p className="arca-native-notice">同步说明：{consumption.reason}</p>}
      {loading && <p className="arca-native-muted">正在读取 SKILL.md 正文…</p>}
      {!loading && error && (
        <p className="arca-native-notice" role="alert" style={{ color: "var(--color-red)" }}>
          读取 SKILL.md 失败：{error}
        </p>
      )}
      {!loading && !error && detail && (
        <>
          {detail.skill_md_truncated && (
            <p className="arca-native-notice">正文内容较长，以下展示已按预览上限截断。</p>
          )}
          <CopyableCodeSection
            heading="SKILL.md 正文"
            code={bodyText || "暂无正文内容。"}
            copyLabel="SKILL.md 正文"
            onCopy={onCopy}
          />
        </>
      )}
    </>
  );

  return {
    title: item?.name || name,
    subtitle: centralSkill ? "中央 Skill 资产" : externalSkill ? "本地外部 Skill" : "Skill 详情",
    leading: <Avatar seed={name} kind="skill" size={32} />,
    body,
    canOpenDefinition: !isExternal && Boolean(centralSkill),
    footerNote: isExternal ? `只读预览 · ${agentName} 外部配置` : `只读预览 · ${agentName}`,
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
      size="wide"
      className="arca-native-dialog"
      title={preview.title}
      subtitle={preview.subtitle}
      leading={preview.leading}
      closeLabel="关闭详情"
      onClose={onClose}
      footerStart={<span>{preview.footerNote}</span>}
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
