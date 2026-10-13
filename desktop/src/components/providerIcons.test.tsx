import { fireEvent, render } from "@testing-library/react";
import { readdir } from "node:fs/promises";
import { resolve } from "node:path";
import { expect, it } from "vitest";
import {
  NAMED_PROVIDER_ICON_IDS,
  ProviderGlyph,
  providerIconUrl,
} from "./providerIcons";

it("bundles one traceable local asset for every named Provider template", async () => {
  const files = await readdir(resolve(process.cwd(), "src/assets/providers"));
  const assetIds = files
    .filter((file) => /\.(png|svg|webp|ico)$/.test(file))
    .map((file) => file.replace(/\.[^.]+$/, ""))
    .sort();

  expect(assetIds).toEqual([...NAMED_PROVIDER_ICON_IDS].sort());
  for (const id of NAMED_PROVIDER_ICON_IDS) {
    expect(providerIconUrl(id), id).toBeTruthy();
  }
});

it("uses a monogram for Custom Providers and retains it behind failed images", () => {
  const custom = render(<ProviderGlyph id="custom" name="Custom Provider" />);
  expect(custom.container.querySelector("img")).not.toBeInTheDocument();
  expect(custom.container.querySelector(".arca-provider-glyph-fallback")).toHaveTextContent("C");
  custom.unmount();

  const branded = render(<ProviderGlyph id="openrouter" name="OpenRouter" />);
  const image = branded.container.querySelector("img");
  expect(image).toBeInTheDocument();
  fireEvent.error(image!);
  expect(image).toHaveStyle({ display: "none" });
  expect(branded.container.querySelector(".arca-provider-glyph-fallback")).toHaveTextContent("O");
});

it("reuses vendor artwork for plan-specific Provider templates", () => {
  expect(providerIconUrl("alibaba-token-plan")).toBe(providerIconUrl("alibaba"));
  expect(providerIconUrl("zai-coding-plan")).toBe(providerIconUrl("zai"));
  expect(providerIconUrl("kimi-for-coding")).toBe(providerIconUrl("moonshotai"));
  expect(providerIconUrl("minimax-coding-plan")).toBeTruthy();

  const minimax = render(
    <ProviderGlyph id="minimax-coding-plan" name="MiniMax Token Plan" />,
  );
  expect(minimax.container.querySelector('[data-provider-icon="minimax"] img'))
    .toBeInTheDocument();
});
