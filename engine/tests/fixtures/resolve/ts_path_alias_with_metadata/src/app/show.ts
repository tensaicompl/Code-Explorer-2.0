import { formatName, Box } from '@/lib/format';
export function show(): string {
  const b = new Box();
  return formatName(' x ') + b.open();
}
