export function run(client: any): number {
  return client.refreshCache();
}

export function control(): number {
  return refreshCache();
}
