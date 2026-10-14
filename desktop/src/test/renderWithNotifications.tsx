import type { ReactNode } from "react";
import { render, type RenderOptions } from "@testing-library/react";
import { ToastProvider } from "../components/Toast";

/** Component fixtures use the same notification host as the real application. */
export function renderWithNotifications(ui: ReactNode, options?: RenderOptions) {
  const Wrapper = options?.wrapper;
  return render(ui, {
    ...options,
    wrapper: ({ children }) => <ToastProvider>
      {Wrapper ? <Wrapper>{children}</Wrapper> : children}
    </ToastProvider>,
  });
}
