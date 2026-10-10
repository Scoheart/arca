import { useEffect } from "react";
import type { SupportedLocale } from ".";
import { localizeLegacyText } from "./legacy";

const localizedAttributes = ["aria-label", "placeholder", "title"] as const;
const textState = new WeakMap<Text, { source: string; rendered: string }>();
const attributeState = new WeakMap<Element, Map<string, { source: string; rendered: string }>>();

function localizeTextNode(node: Text, locale: SupportedLocale) {
  const previous = textState.get(node);
  const source = previous && node.data === previous.rendered ? previous.source : node.data;
  const rendered = localizeLegacyText(source, locale);
  textState.set(node, { source, rendered });
  if (node.data !== rendered) node.data = rendered;
}

function localizeElement(element: Element, locale: SupportedLocale) {
  const states = attributeState.get(element) ?? new Map();
  for (const attribute of localizedAttributes) {
    const current = element.getAttribute(attribute);
    if (current === null) continue;
    const previous = states.get(attribute);
    const source = previous && current === previous.rendered ? previous.source : current;
    const rendered = localizeLegacyText(source, locale);
    states.set(attribute, { source, rendered });
    if (current !== rendered) element.setAttribute(attribute, rendered);
  }
  attributeState.set(element, states);
}

function localizeTree(root: Node, locale: SupportedLocale) {
  if (root.nodeType === Node.TEXT_NODE) {
    localizeTextNode(root as Text, locale);
    return;
  }
  if (root.nodeType !== Node.ELEMENT_NODE && root.nodeType !== Node.DOCUMENT_FRAGMENT_NODE) return;
  if (root.nodeType === Node.ELEMENT_NODE) localizeElement(root as Element, locale);
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    if (walker.currentNode.nodeType === Node.TEXT_NODE) {
      localizeTextNode(walker.currentNode as Text, locale);
    } else {
      localizeElement(walker.currentNode as Element, locale);
    }
  }
}

export function LegacyLocalizationBridge({ locale }: { locale: SupportedLocale }) {
  useEffect(() => {
    // Dialogs, popovers and toasts are portaled to <body>, outside #root.
    const root = document.body;
    localizeTree(root, locale);
    const observer = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        if (mutation.type === "characterData") {
          localizeTextNode(mutation.target as Text, locale);
        } else if (mutation.type === "attributes") {
          localizeElement(mutation.target as Element, locale);
        } else {
          mutation.addedNodes.forEach((node) => localizeTree(node, locale));
        }
      }
    });
    observer.observe(root, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
      attributeFilter: [...localizedAttributes],
    });
    return () => observer.disconnect();
  }, [locale]);
  return null;
}
