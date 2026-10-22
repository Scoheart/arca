import { useRef, type ComponentProps } from "react";

/** WebKit may emit compositionend before the confirming keydown, with
 * isComposing=false but keyCode=229. Never latch the next Enter or use timers.
 */
export function isImeKey(event: { isComposing?: boolean; keyCode?: number }, composing = false) {
  return composing || event.isComposing === true || event.keyCode === 229;
}

/** Candidate keys belong to the input method, not application actions. */
export function ImeInput({ onKeyDown, onCompositionStart, onCompositionEnd, onBlur, ...props }: ComponentProps<"input">) {
  const composing = useRef(false);
  return <input {...props}
    onCompositionStart={event => { composing.current = true; onCompositionStart?.(event); }}
    onCompositionEnd={event => { composing.current = false; onCompositionEnd?.(event); }}
    onBlur={event => { composing.current = false; onBlur?.(event); }}
    onKeyDown={event => {
      if (isImeKey(event.nativeEvent, composing.current)) {
        // Leave an active IME's native candidate confirmation untouched. In
        // WebKit's ended-composition/229 case the candidate is already committed;
        // cancel only the trailing Enter default (including implicit form submit).
        if (!composing.current && !event.nativeEvent.isComposing && event.key === "Enter") {
          event.preventDefault();
        }
        event.stopPropagation();
        return;
      }
      onKeyDown?.(event);
    }} />;
}
