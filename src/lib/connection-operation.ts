export type Service = 'github' | 'notion' | 'obsidian';
export interface ConnectionOperation { service: Service; kind: 'connect' | 'disconnect'; cancelled: boolean }
const operations = new Map<Service, ConnectionOperation>();
const listeners = new Set<() => void>();
const notify = () => listeners.forEach(listener => listener());

export function startOperation(service: Service, kind: ConnectionOperation['kind']): ConnectionOperation | null {
  if (operations.has(service)) return null;
  const operation = { service, kind, cancelled: false };
  operations.set(service, operation);
  notify();
  return operation;
}
export function cancelOperation(operation: ConnectionOperation): boolean {
  if (operations.get(operation.service) !== operation || operation.kind !== 'connect') return false;
  operation.cancelled = true;
  return true;
}
export function isCurrentOperation(operation: ConnectionOperation): boolean {
  return operations.get(operation.service) === operation && !operation.cancelled;
}
export function finishOperation(operation: ConnectionOperation): void {
  if (operations.get(operation.service) !== operation) return;
  operations.delete(operation.service);
  notify();
}
export function currentOperation(service: Service): ConnectionOperation | null { return operations.get(service) ?? null; }
export function subscribeOperations(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
