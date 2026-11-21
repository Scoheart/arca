import { Select } from "@base-ui/react/select";
import type { ReactNode } from "react";
import { ChevronDownIcon } from "./icons";
import { useModalPopupContainer } from "./ui";

export interface FormSelectOption {
  value: string;
  label: string;
  icon?: ReactNode;
}

export function FormSelect({
  ariaLabel,
  value,
  options,
  onChange,
  autoFocus = false,
  placeholder,
  triggerContent,
  title,
  disabled = false,
}: {
  ariaLabel: string;
  value: string;
  options: readonly FormSelectOption[];
  onChange: (value: string) => void;
  autoFocus?: boolean;
  placeholder?: string;
  triggerContent?: ReactNode;
  title?: string;
  disabled?: boolean;
}) {
  const popupContainer = useModalPopupContainer();
  const selected = options.find((option) => option.value === value);

  return (
    <div className="arca-form-select">
      <Select.Root<string>
        modal={false}
        disabled={disabled}
        value={selected ? value : null}
        onValueChange={(next) => {
          if (next !== null) onChange(next);
        }}
      >
        <Select.Trigger
          aria-label={ariaLabel}
          title={title}
          autoFocus={autoFocus}
          className="arca-model-field arca-form-select-trigger"
        >
          <Select.Value className="arca-form-select-value">
            {triggerContent ?? selected?.label ?? placeholder ?? ""}
          </Select.Value>
          <ChevronDownIcon className="arca-form-select-chevron" aria-hidden="true" />
        </Select.Trigger>
        <Select.Portal container={popupContainer}>
          <Select.Positioner
            alignItemWithTrigger={false}
            positionMethod="fixed"
            sideOffset={5}
            align="start"
            className="arca-form-select-positioner"
          >
            <Select.Popup className="arca-form-select-menu" aria-label={ariaLabel}>
              {options.map((option) => (
                <Select.Item key={option.value} value={option.value} label={option.label} className="arca-form-select-option">
                  <Select.ItemText className="arca-form-select-option-content">{option.icon}{option.label}</Select.ItemText>
                  <Select.ItemIndicator className="arca-form-select-option-mark" aria-hidden="true">✓</Select.ItemIndicator>
                </Select.Item>
              ))}
            </Select.Popup>
          </Select.Positioner>
        </Select.Portal>
      </Select.Root>
    </div>
  );
}
