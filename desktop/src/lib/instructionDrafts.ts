import type { InstructionInventory, InstructionSnapshot } from './instructions';
export interface InstructionDraft { snapshot: InstructionSnapshot | null; text: string; creating: boolean; inventory: InstructionInventory }
// Session memory only. A source file remains the sole durable document.
export const instructionDrafts = new Map<string, InstructionDraft>();
