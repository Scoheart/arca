import { useEffect, useRef, useState } from "react";
import { normalizeSkillCommandError } from "../hooks/useSkillsState";
import type {
  InventoryState,
  OperationPlan,
  PlannedSkill,
  SkillCommandError,
  SkillRiskSummary,
  SkillsInventory,
} from "../lib/types";
import { agentName } from "./brandIcons";
import { SkillRiskBadge, skillSourceText } from "./SkillCard";
import { DialogShell } from "./DialogShell";
import { DialogDisclosure } from "./DialogDisclosure";

export interface SkillAssignmentContext {
  enabled: boolean;
  agentIds: string[];
  targetIds: string[];
}

export interface SkillReviewDialogProps {
  plan: OperationPlan;
  assignmentContext?: SkillAssignmentContext;
  onCommit(
    plan: OperationPlan,
    findingsConfirmation: string | null,
  ): Promise<SkillsInventory>;
  onClose(): void | Promise<void>;
  onCommitted(inventory: SkillsInventory): void;
  onRecoveryRequired(message: string): void;
}

const confirmLabels = {
  install: "确认安装",
  import: "确认导入",
  update: "确认更新",
  remove: "确认移除",
  assignment: "确认更改分配",
  repair: "确认修复",
} as const;

const overrideLabels = {
  install: "仍然安装",
  import: "仍然导入",
  update: "仍然更新",
  remove: "仍然移除",
  assignment: "仍然更改分配",
  repair: "仍然修复",
} as const;

const operationLabels = {
  install: "安装",
  import: "导入",
  update: "更新",
  remove: "删除",
  assignment: "分配",
  repair: "修复",
} as const;

const stateLabels: Record<InventoryState, string> = {
  managed: "已托管",
  assigned: "已分配",
  external: "外部副本",
  locally_modified: "本地已修改",
  broken_link: "链接损坏",
  conflicting_link: "链接冲突",
  missing: "正文缺失",
  update_available: "有更新",
};

const fileChangeLabels = {
  added: "新增",
  modified: "修改",
  removed: "删除",
  mode_changed: "权限变化",
  link_changed: "链接变化",
} as const;

const targetStateLabels = {
  missing: "当前不存在",
  managed: "已由 Arca 管理",
  broken: "链接损坏",
  directory: "已有目录",
  unknown_symlink: "未知符号链接",
} as const;

function hashText(hash: string | null) {
  return hash ?? "无";
}

function agentNames(ids: string[]) {
  return [...new Set(ids.map((id) => agentName(id)))].join("、") || "无";
}

function RiskEvidence({ risk }: { risk: SkillRiskSummary }) {
  return (
    <div className="arca-skill-review-findings" data-level={risk.level}>
      <div className="arca-skill-review-section-title">
        <span>风险证据</span>
        <SkillRiskBadge level={risk.level} />
      </div>
      {risk.findings.length === 0 ? (
        <p className="arca-skill-review-empty">未发现需要展示的风险证据。</p>
      ) : (
        <ul>
          {risk.findings.map((finding, index) => (
            <li
              key={`${finding.rule_id}:${finding.path}:${finding.line ?? "file"}:${index}`}
            >
              <div className="arca-skill-review-finding-head">
                <code>
                  {finding.path}
                  {finding.line === null ? "" : `:${finding.line}`}
                </code>
                <SkillRiskBadge level={finding.level} />
              </div>
              <p>{finding.reason}</p>
              <code>
                {finding.rule_id} · v{finding.rule_version}
              </code>
            </li>
          ))}
        </ul>
      )}
      {risk.findings_truncated && (
        <p className="arca-skill-review-truncation">
          已显示 {risk.findings.length} / {risk.finding_count} 条证据
        </p>
      )}
    </div>
  );
}

function PlannedSkillReview({
  skill,
  kind,
}: {
  skill: PlannedSkill;
  kind: OperationPlan["kind"];
}) {
  const replacesCentral =
    skill.replace_existing &&
    (kind === "install" ||
      kind === "import" ||
      kind === "update" ||
      (kind === "repair" && skill.existing_source !== null));
  return (
    <article className="arca-skill-review-skill">
      <header>
        <div>
          <h3>{skill.manifest.name}</h3>
          <p>{skill.manifest.description}</p>
        </div>
        <SkillRiskBadge level={skill.risk.level} />
      </header>

      <DialogDisclosure title="文件与来源详情" summary={`${skill.files.length} 个文件`}>
      <dl className="arca-skill-review-metadata">
        {replacesCentral ? (
          <>
            <div>
              <dt>现有来源</dt>
              <dd>
                {skill.existing_source
                  ? skillSourceText(skill.existing_source)
                  : "未记录的中央副本"}
              </dd>
            </div>
            <div>
              <dt>候选来源</dt>
              <dd>{skillSourceText(skill.source)}</dd>
            </div>
          </>
        ) : (
          <div>
            <dt>来源</dt>
            <dd>{skillSourceText(skill.source)}</dd>
          </div>
        )}
        <div>
          <dt>Revision</dt>
          <dd title={skill.resolved_revision ?? undefined}>
            <code>{skill.resolved_revision ?? "未记录"}</code>
          </dd>
        </div>
        <div>
          <dt>内容哈希</dt>
          <dd title={skill.content_hash}>
            <code>{skill.content_hash}</code>
          </dd>
        </div>
        {skill.existing_states.length > 0 && (
          <div>
            <dt>现有状态</dt>
            <dd>{skill.existing_states.map((state) => stateLabels[state]).join("、")}</dd>
          </div>
        )}
        {replacesCentral && (
          <div>
            <dt>冲突处理</dt>
            <dd>先在 ~/.arca/backups/skills/ 保留备份，再替换现有中央副本</dd>
          </div>
        )}
      </dl>

      <section className="arca-skill-review-files" aria-label={`${skill.manifest.name} 文件变更`}>
        <h4>文件变更</h4>
        <ul>
          {skill.files.map((file) => (
            <li key={file.path}>
              <div className="arca-skill-review-file-head">
                <code>{file.path}</code>
                <span>{fileChangeLabels[file.kind]}</span>
              </div>
              <dl>
                <div>
                  <dt>变更前</dt>
                  <dd><code>{hashText(file.before_hash)}</code></dd>
                </div>
                <div>
                  <dt>变更后</dt>
                  <dd><code>{hashText(file.after_hash)}</code></dd>
                </div>
              </dl>
              {file.diff_truncated ? (
                <p className="arca-skill-review-truncation">文本差异已截断</p>
              ) : file.unified_diff ? (
                <pre aria-label={`${file.path} 文本差异`}>{file.unified_diff}</pre>
              ) : null}
            </li>
          ))}
        </ul>
      </section>

      </DialogDisclosure>

      {skill.risk.findings.length > 0 && <RiskEvidence risk={skill.risk} />}
    </article>
  );
}

export function SkillReviewDialog({
  plan,
  assignmentContext,
  onCommit,
  onClose,
  onCommitted,
  onRecoveryRequired,
}: SkillReviewDialogProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SkillCommandError | null>(null);
  const [reviewExpired, setReviewExpired] = useState(false);
  const [riskHash, setRiskHash] = useState<string | null>(null);
  const [riskAcknowledged, setRiskAcknowledged] = useState(false);
  const commitInFlight = useRef(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const closeReview = () => {
    if (commitInFlight.current) return;
    void onClose();
  };

  const submit = async (findingsConfirmation: string | null) => {
    if (commitInFlight.current) return;
    commitInFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      const inventory = await onCommit(plan, findingsConfirmation);
      if (!mounted.current) return;
      setRiskHash(null);
      setRiskAcknowledged(false);
      onCommitted(inventory);
    } catch (reason) {
      if (!mounted.current) return;
      const nextError = normalizeSkillCommandError(reason);
      if (
        nextError.code === "confirmation_required" &&
        nextError.findings_hash &&
        nextError.findings_hash === plan.findings_hash
      ) {
        setRiskHash(nextError.findings_hash);
        setRiskAcknowledged(false);
      } else if (
        nextError.code === "confirmation_required" &&
        !nextError.findings_hash
      ) {
        setRiskHash(null);
        setRiskAcknowledged(false);
        setReviewExpired(true);
        setError({
          code: "protocol_error",
          message: "风险确认信息不完整，请重新操作。",
        });
      } else if (nextError.code === "confirmation_required") {
        setRiskHash(null);
        setRiskAcknowledged(false);
        setReviewExpired(true);
        setError({
          code: "confirmation_mismatch",
          message: "风险内容已变化，请重新操作。",
        });
      } else if (nextError.code === "plan_stale") {
        setRiskHash(null);
        setRiskAcknowledged(false);
        setReviewExpired(true);
        setError({
          code: "plan_stale",
          message: "确认已过期，请重新操作。",
        });
      } else if (nextError.code === "recovery_required") {
        setError(nextError);
        onRecoveryRequired(nextError.message);
      } else {
        setError(nextError);
      }
    } finally {
      commitInFlight.current = false;
      if (mounted.current) setBusy(false);
    }
  };

  const needsRiskAcknowledgment = plan.requires_risk_override || riskHash !== null;
  const assignmentTargetIds = new Set(assignmentContext?.targetIds ?? []);
  const assignmentTargets = plan.targets.filter((target) =>
    assignmentTargetIds.has(target.target_id),
  );

  return (
    <DialogShell
      className="arca-dialog-skill-review"
      key="review"
      kind="review"
      size="lg"
      title="确认 Skill 更改"
      subtitle={`${operationLabels[plan.kind]} · ${plan.skills.length} 个 Skill`}
      busy={busy}
      onClose={closeReview}
      footerEnd={
        <>
          <button type="button" className="btn-ghost" disabled={busy} onClick={closeReview}>取消</button>
          <button
            type="button"
            className={needsRiskAcknowledgment ? "btn-danger" : "btn-primary"}
            disabled={busy || reviewExpired || needsRiskAcknowledgment && !riskAcknowledged}
            onClick={() => void submit(needsRiskAcknowledgment ? plan.findings_hash : null)}
          >
            {busy ? "正在提交…" : needsRiskAcknowledgment ? overrideLabels[plan.kind] : confirmLabels[plan.kind]}
          </button>
        </>
      }
    >
      <div className="arca-skill-review-dialog">
        <div className="arca-skill-review-body">
          {error && <p className="arca-skill-review-error" role="alert">{error.message}</p>}

          <section className="arca-skill-review-section" aria-label="Skill 变更">
            {plan.skills.map((skill) => (
              <PlannedSkillReview
                key={`${skill.manifest.name}:${skill.content_hash}`}
                skill={skill}
                kind={plan.kind}
              />
            ))}
          </section>

          {needsRiskAcknowledgment && (
            <label className="arca-skill-risk-acknowledgment">
              <input type="checkbox" checked={riskAcknowledged} disabled={busy || reviewExpired}
                onChange={(event) => setRiskAcknowledged(event.target.checked)} />
              <span>我已了解高风险内容及其影响</span>
            </label>
          )}

          {plan.kind === "assignment" && assignmentContext && (
            <section
              className="arca-skill-review-section arca-skill-review-assignment"
              aria-label="分配影响"
            >
              <h3>
                {assignmentContext.enabled ? "将为" : "将停止为"}{" "}
                {agentNames(assignmentContext.agentIds)} 分配
              </h3>
              <ul>
                {assignmentTargets.map((target) => (
                  <li key={target.target_id}>
                    <code>{target.global_dir}</code>
                    <span>
                      {agentNames(target.affected_agent_ids)}{" "}
                      {assignmentContext.enabled ? "将共享此目标" : "将失去访问"}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          )}

          <DialogDisclosure title="目标与 Agent 影响" summary={`${plan.targets.length} 个目标`}>
          <section className="arca-skill-review-section arca-skill-review-targets">
            <div className="arca-skill-review-section-title">
              <h3>目标与 Agent 影响</h3>
              <span>{plan.targets.length} 个目标</span>
            </div>
            {plan.targets.length === 0 ? (
              <p className="arca-skill-review-empty">此操作不会更改 Agent 目标。</p>
            ) : (
              <ul>
                {plan.targets.map((target) => (
                  <li key={target.target_id}>
                    <code>{target.global_dir}</code>
                    <span>{targetStateLabels[target.expected]}</span>
                    <dl>
                      <div>
                        <dt>主要 Agent</dt>
                        <dd>{agentNames(target.primary_agent_ids)}</dd>
                      </div>
                      <div>
                        <dt>受影响 Agent</dt>
                        <dd>{agentNames(target.affected_agent_ids)}</dd>
                      </div>
                    </dl>
                  </li>
                ))}
              </ul>
            )}
          </section>
          </DialogDisclosure>

          {plan.warnings.length > 0 && (
            <section className="arca-skill-review-section arca-skill-review-warnings">
              <h3>注意事项</h3>
              <ul>
                {plan.warnings.map((warning, index) => (
                  <li key={`${warning}:${index}`}>{warning}</li>
                ))}
              </ul>
            </section>
          )}
        </div>

      </div>


    </DialogShell>
  );
}
