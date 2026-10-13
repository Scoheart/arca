import { useState, type ReactNode } from "react";
import { ChevronDownIcon } from "./icons";

/** Secondary fields stay mounted so collapsing never discards an edit. */
export function DialogDisclosure({ title, summary, children, invalid = false, defaultOpen = false }: {
  title: string; summary?: ReactNode; children: ReactNode; invalid?: boolean; defaultOpen?: boolean;
}) {
  const [expanded, setExpanded] = useState(defaultOpen);
  return <details className="arca-dialog-disclosure" open={expanded || invalid}
    onToggle={(event) => setExpanded(event.currentTarget.open)}>
    <summary><span>{title}</span>{summary != null && <small>{summary}</small>}<ChevronDownIcon aria-hidden="true" className="w-3.5 h-3.5" /></summary>
    <div className="arca-dialog-disclosure-body">{children}</div>
  </details>;
}
