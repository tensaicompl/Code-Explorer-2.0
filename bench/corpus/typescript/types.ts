/** Type-level programming: mapped, conditional and template literal types. */
export type Primitive = string | number | boolean | bigint | symbol | null | undefined;

export type DeepReadonly<T> = T extends Primitive
  ? T
  : T extends Array<infer U>
    ? ReadonlyArray<DeepReadonly<U>>
    : { readonly [K in keyof T]: DeepReadonly<T[K]> };

export type Getters<T> = {
  [K in keyof T as `get${Capitalize<string & K>}`]: () => T[K];
};

export type PathOf<T, Prefix extends string = ""> = {
  [K in keyof T & string]: T[K] extends object
    ? `${Prefix}${K}` | PathOf<T[K], `${Prefix}${K}.`>
    : `${Prefix}${K}`;
}[keyof T & string];

export interface Settings {
  server: { host: string; port: number; tls: { enabled: boolean } };
  features: string[];
}

export type SettingPath = PathOf<Settings>;

export function getPath<T, P extends string>(obj: T, path: P): unknown {
  return path.split(".").reduce<unknown>((acc, key) => {
    if (acc && typeof acc === "object") {
      return (acc as Record<string, unknown>)[key];
    }
    return undefined;
  }, obj);
}

export const defaults: DeepReadonly<Settings> = {
  server: { host: "localhost", port: 8080, tls: { enabled: false } },
  features: ["search", "map"],
};

export function assertNever(x: never): never {
  throw new Error("unexpected value: " + JSON.stringify(x));
}

export type Tuple = [first: string, second?: number, ...rest: boolean[]];

export const pick = <T extends object, K extends keyof T>(obj: T, ...keys: K[]): Pick<T, K> =>
  keys.reduce((out, k) => ({ ...out, [k]: obj[k] }), {} as Pick<T, K>);
