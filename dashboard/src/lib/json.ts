
export type Json = Record<string, unknown>

export const fail = (path: string, expected: string): never => {
  throw new Error(`${path} should be ${expected}`)
}
export const isObject = (value: unknown): value is Json =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
export const object = (value: unknown, path: string): Json => (isObject(value) ? value : fail(path, 'an object'))
export const num = (value: unknown, path: string): number =>
  typeof value === 'number' && Number.isFinite(value) ? value : fail(path, 'a number')
export const optNum = (value: unknown, path: string): number | null => (value == null ? null : num(value, path))
export const bool = (value: unknown, path: string): boolean =>
  typeof value === 'boolean' ? value : fail(path, 'true or false')
export const str = (value: unknown, path: string): string => (typeof value === 'string' ? value : fail(path, 'text'))
export const list = <T>(value: unknown, path: string, item: (entry: unknown, path: string) => T): T[] =>
  value == null ? [] : Array.isArray(value) ? value.map((entry, i) => item(entry, `${path}[${i}]`)) : fail(path, 'a list')
export const requiredList = <T>(value: unknown, path: string, item: (entry: unknown, path: string) => T): T[] =>
  Array.isArray(value) ? list(value, path, item) : fail(path, 'a list')

export function describeErrors<T>(what: string, parse: () => T): T {
  try {
    return parse()
  } catch (error) {
    throw new Error(`${what}: ${error instanceof Error ? error.message : String(error)}.`)
  }
}
