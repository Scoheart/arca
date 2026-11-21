import { supportsEffort, supportsSampling } from "../api";
import { letter, protocols, type ModelTarget, type Parameters, type Protocol, type RunDraft } from "../types";

const parameterHints: Record<Protocol, string> = {
  "openai-completions": '例如 {"thinking":{"type":"enabled"}}。以该 Provider 支持为准。',
  "openai-responses": '例如 {"reasoning":{"effort":"high"}}。同一个参数只填一次。',
  "anthropic-messages": '例如 {"thinking":{"type":"adaptive"}}。具体参数因模型而异。',
  "gemini-interactions": '例如 {"generation_config":{"thinking_level":"high"}}。',
  "gemini-generate-content": '例如 {"generationConfig":{"thinkingConfig":{"thinkingLevel":"HIGH"}}}。',
  "aws-bedrock-converse": "可用 additionalModelRequestFields 设置模型的原生参数。",
};

interface Props {
  open: boolean;
  models: ModelTarget[];
  drafts: RunDraft[];
  selected: RunDraft | undefined;
  profile: ModelTarget | undefined;
  instructions: string;
  loading: boolean;
  running: boolean;
  canRun: boolean;
  canAdd: boolean;
  canRefresh: boolean;
  onClose(): void;
  onRefresh(): void;
  onAdd(): void;
  onSelect(id: number): void;
  onChangeProfile(id: string): void;
  onChangeParameter<K extends keyof Parameters>(name: K, value: Parameters[K]): void;
  onInstructions(value: string): void;
  onDuplicate(): void;
  onRemove(): void;
  onRun(): void;
  onPreview(): void;
}

export function RunSettings(props: Props) {
  const { selected, profile, loading, running, canAdd, models } = props;
  return (
    <aside className="settings" id="run-settings" aria-labelledby="settings-title" hidden={!props.open}>
      <header className="settings-bar">
        <h2 id="settings-title">运行设置</h2>
        <div className="settings-bar-actions">
          <button type="button" className="tool-button" disabled={!props.canRun} onClick={props.onPreview}>请求预览</button>
          <button type="button" className="icon-button" id="close-settings" title="关闭运行设置"
            aria-label="关闭运行设置" onClick={props.onClose}>×</button>
        </div>
      </header>
      <div className="settings-body">
        <div className="set-heading">
          <span>对照请求</span>
          <button type="button" className="tool-button refresh" id="refresh" disabled={!props.canRefresh}
            onClick={props.onRefresh}>刷新模型</button>
        </div>
        <div className="profile-tabs" role="group" aria-label="选择要调整的请求">
          {props.drafts.map((draft, index) => (
            <button key={draft.id} className="tab-button" type="button" aria-pressed={draft.id === selected?.id}
              aria-label={"配置请求 " + letter(index)} onClick={() => props.onSelect(draft.id)}>
              {letter(index)}
            </button>
          ))}
          <button type="button" className="tab-button" id="add" aria-label="添加对照请求" title="添加对照请求"
            disabled={!canAdd} onClick={props.onAdd}>+</button>
        </div>

        {selected ? (
          <div className="target-editor">
            <label className="model-label">模型
              <select className="model-select" value={selected.profileId} disabled={loading || running || !models.length}
                onChange={(event) => props.onChangeProfile(event.target.value)}>
                <option value="">选择已保存的模型</option>
                {models.map((model) => <option key={model.id} value={model.id}>
                  {[model.name, model.provider !== model.name ? model.provider : "", protocols[model.protocol]].filter(Boolean).join(" · ")}
                </option>)}
              </select>
            </label>
            {profile && (
              <div className="target-details" aria-live="polite">
                <span className="target-model">{profile.model}</span>
                <span className="target-protocol">{protocols[profile.protocol]}</span>
                <span className="target-origin">{profile.origin}</span>
              </div>
            )}
            <div className="target-actions">
              <button type="button" className="tool-button duplicate" disabled={!canAdd} onClick={props.onDuplicate}>复制此项</button>
              <button type="button" className="tool-button remove" disabled={running || props.drafts.length <= 1}
                onClick={props.onRemove}>移除</button>
              <button type="button" className="tool-button run-one" disabled={!props.canRun} onClick={props.onRun}>只运行此项</button>
            </div>
          </div>
        ) : (
          <p className="selection-empty">{loading ? "正在读取模型…" : "在 Arca 里保存一个模型，再刷新列表。"}</p>
        )}

        <details className="system">
          <summary>系统指令</summary>
          <label className="sr-only" htmlFor="instructions">共同系统指令</label>
          <textarea id="instructions" rows={4} maxLength={16384} disabled={running}
            placeholder="可选。对这一轮的所有请求生效。" value={props.instructions}
            onChange={(event) => props.onInstructions(event.target.value)} />
        </details>

        {selected && (
          <ParameterEditor key={selected.id} profile={profile} parameters={selected.parameters} disabled={!profile || running}
            onChange={props.onChangeParameter} />
        )}
      </div>
    </aside>
  );
}

interface EditorProps {
  profile?: ModelTarget;
  parameters: Parameters;
  disabled: boolean;
  onChange<K extends keyof Parameters>(name: K, value: Parameters[K]): void;
}

function ParameterEditor({ profile, parameters: p, disabled, onChange }: EditorProps) {
  const sampling = supportsSampling(profile);
  const effort = supportsEffort(profile);
  const canStream = !!profile?.streaming_available;
  return (
    <div className="parameter-editor">
      <section className="section response-mode">
        <label>返回方式
          <select name="streaming" value={p.streaming && canStream ? "live" : "full"}
            disabled={disabled || !canStream} onChange={(event) => onChange("streaming", event.target.value === "live")}>
            <option value="live">逐步显示</option>
            <option value="full">完整返回</option>
          </select>
        </label>
        {profile && !canStream && <p className="help">这个端点使用完整返回。</p>}
      </section>
      <section className="section reasoning">
        {effort ? (
          <label>推理强度
            <select name="reasoning_effort" value={p.reasoning_effort} disabled={disabled}
              onChange={(event) => onChange("reasoning_effort", event.target.value)}>
              <option value="">服务默认</option>
              <option value="none">无 · none</option>
              <option value="minimal">极低 · minimal</option>
              <option value="low">低 · low</option>
              <option value="medium">中 · medium</option>
              <option value="high">高 · high</option>
              <option value="xhigh">极高 · xhigh</option>
              <option value="max">最高 · max</option>
            </select>
          </label>
        ) : (
          <>
            <label>推理设置</label>
            <p className="help">此协议的推理参数在高级设置中填写。</p>
          </>
        )}
      </section>
      <section className="section sampling" aria-label="采样和输出">
        {sampling && (
          <div className="field-pair">
            <label>Temperature
              <input name="temperature" type="number" inputMode="decimal" step="any" min={0} max={2}
                placeholder="默认" value={p.temperature} disabled={disabled}
                onChange={(event) => onChange("temperature", event.target.value)} />
            </label>
            <label>Top P
              <input name="top_p" type="number" inputMode="decimal" step="any" min={0} max={1}
                placeholder="默认" value={p.top_p} disabled={disabled}
                onChange={(event) => onChange("top_p", event.target.value)} />
            </label>
          </div>
        )}
        <label>最大输出 Token
          <input name="max_output_tokens" type="number" inputMode="numeric" step={1} min={1} max={131072}
            placeholder="默认" value={p.max_output_tokens} disabled={disabled}
            onChange={(event) => onChange("max_output_tokens", event.target.value)} />
        </label>
      </section>
      <details className="section advanced">
        <summary>高级设置</summary>
        <label className="json-label">高级 JSON
          <textarea name="extra" rows={5} maxLength={32768} placeholder="{}" spellCheck={false}
            value={p.extra} disabled={disabled} onChange={(event) => onChange("extra", event.target.value)} />
        </label>
        <p className="help parameter-hint">{profile ? parameterHints[profile.protocol] : "先选择模型。"}</p>
      </details>
    </div>
  );
}
