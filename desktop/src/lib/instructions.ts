import { invoke } from '@tauri-apps/api/core';
export interface InstructionFile {
  id: string; path: string; name: string; purpose: string; loading: string; bytes: number; editable: boolean;
  readers: { id: string; name: string }[];
}
export interface InstructionInventory {
  agent_id: string; supported: boolean; revision: string; files: InstructionFile[];
  create: { path: string; initial_text: string } | null; complete: boolean; issues: string[];
  reload: string; native_notes: string; docs: string[];
}
export interface InstructionSnapshot { file: InstructionFile; text: string; revision: string; reload: string }
export interface InstructionPlan { operation_id: string; candidate_hash: string; path: string; creating: boolean }
export const instructionList = (agentId: string) => invoke<InstructionInventory>('instruction_list', { agentId });
export const instructionRead = (agentId: string, fileId: string) => invoke<InstructionSnapshot>('instruction_read', { agentId, fileId });
export const instructionPrepareSave = (agentId: string, fileId: string, expectedRevision: string, text: string) => invoke<InstructionPlan>('instruction_prepare_save', { agentId, fileId, expectedRevision, text });
export const instructionPrepareCreate = (agentId: string, inventoryRevision: string, text: string) => invoke<InstructionPlan>('instruction_prepare_create', { agentId, inventoryRevision, text });
export const instructionCommit = (plan: InstructionPlan) => invoke<InstructionSnapshot>('instruction_commit', { operationId: plan.operation_id, candidateHash: plan.candidate_hash });
