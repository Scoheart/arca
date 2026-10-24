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
  "agnes-ai-cn": "agnes-ai",
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
  "0g-private-computer",
  "302-ai",
  "agnes-ai",
  "ai21",
  "aihubmix",
  "akashml",
  "alibaba",
  "amazon-bedrock-mantle",
  "ant-ling",
  "antdigital-dtmaas",
  "anthropic",
  "apinex",
  "atria",
  "azure-openai",
  "baidu-qianfan",
  "baseten",
  "beatapi",
  "bilibili-index-translate",
  "byteplus",
  "cerebras",
  "chutes",
  "clean-apis",
  "cloudflare-workers-ai",
  "codecraft",
  "cohere",
  "constellation-gate",
  "coralbricks",
  "deepinfra",
  "deepseek",
  "digitalocean",
  "dolphin",
  "experiential-labs",
  "featherless-ai",
  "fikra",
  "fireworks",
  "flock",
  "friendli",
  "github-models",
  "gmi-cloud",
  "gonkarouter",
  "google",
  "groq",
  "helicone",
  "huggingface",
  "iflytek-spark-x2",
  "inception",
  "indicstack",
  "inference-net",
  "inferx",
  "infini-ai",
  "infron",
  "io-net",
  "iteracompute",
  "krafton-raon",
  "liquid-ai",
  "llmtr",
  "lm-studio",
  "longcat",
  "maximo-ai",
  "meta",
  "mistral",
  "modelscope",
  "moonshotai",
  "munito",
  "nanogpt",
  "nebius",
  "nomask",
  "nous-portal",
  "novita-ai",
  "nscale",
  "nvidia",
  "ollama",
  "openai",
  "openrouter",
  "orcarouter",
  "ovhcloud",
  "oxion-ai",
  "perplexity",
  "poe",
  "portkey",
  "ppio",
  "ppq",
  "qiniu-ai",
  "requesty",
  "river",
  "routeway",
  "runanywhere",
  "runinfra",
  "sambanova",
  "sarashina",
  "scaleway",
  "scnet",
  "sensenova",
  "siliconflow",
  "stepfun",
  "synthetic",
  "tencent",
  "tinfoil",
  "together",
  "tokenharbor",
  "trustedrouter",
  "unisound",
  "unorouter",
  "upstage",
  "venice",
  "vercel-ai-gateway",
  "vllm",
  "volcengine",
  "wandb",
  "xai",
  "xiaomi",
  "yeta",
  "zai",
  "zenmux",
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
