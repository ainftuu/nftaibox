import { describe, expect, it } from "vitest";
import { chooseModel, compatibleKeys, type PortalKey } from "./portal";
describe("NFTAI guided configuration", () => {
  const key = (id: number, protocol: string, usable = true): PortalKey => ({
    id,
    protocol,
    usable,
    name: "key",
    group: "group",
    status: "active",
    quota: 0,
    quota_used: 0,
    expires_at: null,
  });
  it("never suggests an incompatible or disabled key", () => {
    const keys = [
      key(1, "openai-completions"),
      key(2, "anthropic-messages"),
      key(3, "openai-responses"),
      key(4, "anthropic-messages", false),
      key(5, "unknown"),
      { ...key(6, "openai-responses"), codex_cli_only: true },
      { ...key(7, "anthropic-messages"), claude_code_only: true },
    ];
    expect(compatibleKeys("claude", keys).map((k) => k.id)).toEqual([1, 2, 3, 7]);
    expect(compatibleKeys("codex", keys).map((k) => k.id)).toEqual([1, 2, 3, 6]);
    expect(compatibleKeys("dsh", keys).map((k) => k.id)).toEqual([1, 2, 3]);
    expect(compatibleKeys("opencode", keys).map((k) => k.id)).toEqual([1, 2, 3]);
  });
  it("recommends only models actually returned by the gateway", () => {
    expect(chooseModel("dsh", ["custom-x", "deepseek-chat"])).toBe(
      "deepseek-chat",
    );
    expect(chooseModel("codex", ["custom-x"])).toBe("custom-x");
    expect(chooseModel("claude", [])).toBe("");
  });
});
