import { cleanup, screen } from "@testing-library/react";
import { renderWithNotifications as render } from "../test/renderWithNotifications";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProxySettingsDialog } from "./ProxySettingsDialog";

afterEach(cleanup);

describe("ProxySettingsDialog", () => {
  it("saves a trimmed HTTP proxy", async () => {
    const onSave = vi.fn().mockResolvedValue({ proxy_url: "http://127.0.0.1:7890" });
    const onClose = vi.fn();
    render(<ProxySettingsDialog proxyUrl={null} onClose={onClose} onSave={onSave} />);

    await userEvent.type(screen.getByLabelText("代理地址"), "  http://127.0.0.1:7890  ");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));

    expect(onSave).toHaveBeenCalledWith("http://127.0.0.1:7890");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("uses an empty value to disable the proxy", async () => {
    const onSave = vi.fn().mockResolvedValue({ proxy_url: null });
    render(
      <ProxySettingsDialog
        proxyUrl="http://127.0.0.1:7890"
        onClose={() => {}}
        onSave={onSave}
      />,
    );

    await userEvent.clear(screen.getByLabelText("代理地址"));
    await userEvent.click(screen.getByRole("button", { name: "保存" }));

    expect(onSave).toHaveBeenCalledWith(null);
  });

  it("saves a SOCKS5 proxy without adding another mode control", async () => {
    const onSave = vi.fn().mockResolvedValue({ proxy_url: "socks5://127.0.0.1:7891" });
    render(<ProxySettingsDialog proxyUrl={null} onClose={() => {}} onSave={onSave} />);

    expect(screen.getByText("HTTP · SOCKS4 · SOCKS5")).toBeVisible();
    await userEvent.type(screen.getByLabelText("代理地址"), "socks5://127.0.0.1:7891");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));

    expect(onSave).toHaveBeenCalledWith("socks5://127.0.0.1:7891");
  });

  it("keeps the dialog open and shows a validation error", async () => {
    const onClose = vi.fn();
    const onSave = vi.fn().mockRejectedValue("支持 HTTP、SOCKS4 和 SOCKS5 代理。");
    render(<ProxySettingsDialog proxyUrl={null} onClose={onClose} onSave={onSave} />);

    await userEvent.type(screen.getByLabelText("代理地址"), "https://127.0.0.1:7890");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("支持 HTTP、SOCKS4 和 SOCKS5 代理。");
    expect(onClose).not.toHaveBeenCalled();
  });
});
