import type { KeyboardEvent, MouseEvent, ReactNode } from "react";
import { Avatar } from "./ui";

export type ResourceCardAttention = "warning" | "danger" | "shadowed";
export type ResourceKind = "model" | "mcp" | "skill";

export function ResourceKindIcon({
  kind,
  seed,
  icon,
}: {
  kind: ResourceKind;
  seed: string;
  icon?: ReactNode;
}) {
  return <Avatar seed={seed} kind={kind} icon={icon} size={34} />;
}

export function ResourceCard({
  className,
  identity,
  configuration,
  state,
  impact,
  selected = false,
  attention,
  ariaLabel,
  onOpen,
}: {
  className?: string;
  identity: ReactNode;
  configuration?: ReactNode;
  state?: ReactNode;
  impact?: ReactNode;
  selected?: boolean;
  attention?: ResourceCardAttention;
  ariaLabel: string;
  onOpen: () => void;
}) {
  const openFromKeyboard = (event: KeyboardEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return;
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    onOpen();
  };

  const openFromPointer = (event: MouseEvent<HTMLElement>) => {
    if ((event.target as HTMLElement).closest("button, a, input, select, textarea, summary")) {
      return;
    }
    onOpen();
  };

  return (
    <article
      className={`arca-resource-card${className ? ` ${className}` : ""}`}
      role="button"
      tabIndex={0}
      aria-label={ariaLabel}
      aria-pressed={selected}
      data-selected={selected ? "true" : undefined}
      data-attention={attention}
      onClick={openFromPointer}
      onKeyDown={openFromKeyboard}
    >
      <header className="arca-resource-card-identity" data-resource-slot="identity">
        {identity}
      </header>
      {configuration != null && (
        <div className="arca-resource-card-configuration" data-resource-slot="configuration">
          {configuration}
        </div>
      )}
      {state != null && (
        <div className="arca-resource-card-state" data-resource-slot="state">
          {state}
        </div>
      )}
      {impact != null && (
        <footer className="arca-resource-card-impact" data-resource-slot="impact">
          {impact}
        </footer>
      )}
    </article>
  );
}
