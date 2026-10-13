import minimaxIconUrl from "../assets/agents/minimax-code.png";
import opencodeIconUrl from "../assets/agents/opencode.svg";
import kiloIconUrl from "../assets/agents/kilo-code.svg";

const providerIconModules = import.meta.glob("../assets/providers/*.{png,svg,webp,ico}", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;

const EXTRA_PROVIDER_LOGOS: Record<string, string> = {
  minimax: minimaxIconUrl,
  opencode: opencodeIconUrl,
  "kilo-gateway": kiloIconUrl,
};

const PROVIDER_ICON_ALIASES: Record<string, string> = {
  "opencode-zen": "opencode",
  "opencode-go": "opencode",
  "tencent-tokenhub-cn": "tencent",
  "tencent-tokenhub-global": "tencent",
  "ollama-cloud": "ollama",
  "tinfoil-proxy": "tinfoil",
  "minimax-cn": "minimax",
  "stepfun-global": "stepfun",
  "zhipuai": "zai",
  "moonshotai-cn": "moonshotai",
  "alibaba-international": "alibaba",
  "siliconflow-cn": "siliconflow",
  "volcengine-coding-plan": "volcengine",
  "baidu-qianfan-coding-plan": "baidu-qianfan",
  "alibaba-coding-plan-cn": "alibaba",
  "alibaba-coding-plan": "alibaba",
  "alibaba-token-plan-cn": "alibaba",
  "alibaba-token-plan": "alibaba",
  "xiaomi-token-plan-cn": "xiaomi",
  "xiaomi-token-plan-sgp": "xiaomi",
  "xiaomi-token-plan-ams": "xiaomi",
  "kimi-for-coding": "moonshotai",
  "zai-coding-plan": "zai",
  "zhipuai-coding-plan": "zai",
  "minimax-coding-plan": "minimax",
  "minimax-cn-coding-plan": "minimax",
  "stepfun-step-plan": "stepfun",
  "stepfun-ai-step-plan": "stepfun",
  "tencent-coding-plan": "tencent",
  "tencent-token-plan": "tencent",
  "tencent-token-plan-global": "tencent",
};

const PROVIDER_LOGOS = Object.fromEntries(
  Object.entries(providerIconModules).map(([path, url]) => [
    path.split("/").pop()!.replace(/\.[^.]+$/, ""),
    url,
  ]),
) as Record<string, string>;

export const NAMED_PROVIDER_ICON_IDS = [
  "portkey",
  "helicone",
  "ppio",
  "infini-ai",
  "aihubmix",
  "302-ai",
  "nanogpt",
  "synthetic",
  "chutes",
  "featherless-ai",
  "venice",
  "friendli",
  "inference-net",
  "ovhcloud",
  "upstage",
  "ai21",
  "zenmux",
  "byteplus",
  "longcat",
  "iflytek-spark-x2",
  "nscale",
  "io-net",
  "inception",
  "tinfoil",
  "azure-openai",
  "amazon-bedrock-mantle",
  "cloudflare-workers-ai",
  "deepinfra",
  "gmi-cloud",
  "sambanova",
  "vercel-ai-gateway",
  "stepfun",
  "perplexity",
  "volcengine",
  "baidu-qianfan",
  "openrouter",
  "anthropic",
  "openai",
  "google",
  "xai",
  "mistral",
  "cohere",
  "deepseek",
  "groq",
  "alibaba",
  "xiaomi",
  "moonshotai",
  "zai",
  "nvidia",
  "cerebras",
  "siliconflow",
  "together",
  "fireworks",
  "poe",
  "huggingface",
  "github-models",
  "novita-ai",
  "qiniu-ai",
  "digitalocean",
  "modelscope",
  "scaleway",
  "nebius",
  "requesty",
  "baseten",
  "wandb",
  "ollama",
  "lm-studio",
  "vllm",
] as const;

const FALLBACK_COLORS = ["#3568D4", "#16856B", "#B84A62", "#9A6618", "#5E55B8", "#277B91"];

function fallbackColor(id: string) {
  let hash = 0;
  for (const char of id) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return FALLBACK_COLORS[hash % FALLBACK_COLORS.length];
}

export function providerIconUrl(id: string): string | undefined {
  const iconId = PROVIDER_ICON_ALIASES[id] ?? id;
  return PROVIDER_LOGOS[iconId] ?? EXTRA_PROVIDER_LOGOS[iconId];
}

export function ProviderGlyph({
  id,
  name,
  size = 26,
}: {
  id: string;
  name?: string;
  size?: number;
}) {
  const iconId = PROVIDER_ICON_ALIASES[id] ?? id;
  const logo = providerIconUrl(id);
  const label = (name || id).trim()[0]?.toUpperCase() ?? "?";
  const radius = Math.round(size * 0.28);

  return (
    <span
      aria-hidden="true"
      data-provider-icon={logo ? iconId : "fallback"}
      className="arca-provider-glyph"
      style={{
        width: size,
        height: size,
        borderRadius: radius,
        background: logo ? "#fff" : fallbackColor(id),
      }}
    >
      <span className="arca-provider-glyph-fallback" style={{ fontSize: Math.round(size * 0.46) }}>
        {label}
      </span>
      {logo && (
        <img
          src={logo}
          alt=""
          draggable={false}
          onError={(event) => { event.currentTarget.style.display = "none"; }}
        />
      )}
    </span>
  );
}
