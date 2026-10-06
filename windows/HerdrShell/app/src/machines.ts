export function cycleMachine(names: readonly string[], current: string, direction: 1 | -1): string | null {
  if (!names.length) return null;
  const index = names.indexOf(current);
  if (index < 0) return direction === 1 ? names[0] : names[names.length - 1];
  return names[(index + direction + names.length) % names.length];
}
