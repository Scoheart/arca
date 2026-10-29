import type { NativeSkillObservation } from "../lib/types";

export const nativeSkillFixture = (overrides: Partial<NativeSkillObservation> = {}): NativeSkillObservation => ({
  id: "cursor:fixture:revision-a:review",
  agent_id: "cursor",
  name: "review",
  plugin_id: "fixture@marketplace",
  plugin_name: "Review tools",
  plugin_version: "1.0.0",
  plugin_revision: "revision-a",
  plugin_enabled: true,
  presence: "cache_only",
  source_path: "~/.cursor/plugins/cache/fixture/revision-a/skills/review/SKILL.md",
  source_fingerprint: "fixture-skill-fingerprint",
  native_id: "review-native-id",
  docs_url: "https://cursor.com/docs/plugins",
  ...overrides,
});
