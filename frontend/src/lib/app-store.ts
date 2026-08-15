import { invoke } from '@tauri-apps/api/core';
import {
  load,
  type Store,
  type StoreOptions,
} from '@tauri-apps/plugin-store';

const resolvedPaths = new Map<string, Promise<string>>();

function resolveStorePath(name: string): Promise<string> {
  let path = resolvedPaths.get(name);
  if (!path) {
    path = invoke<string>('get_app_store_path', { name });
    resolvedPaths.set(name, path);
  }
  return path;
}

/** Load an allowlisted application store from the stable local-data root. */
export async function loadAppStore(
  name: string,
  options?: StoreOptions,
): Promise<Store> {
  return load(await resolveStorePath(name), options);
}
