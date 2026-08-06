import { invoke } from "@tauri-apps/api/core";

const RELAY = import.meta.env.VITE_RELAY ?? "http://127.0.0.1:8787";
const TOKEN_KEY = "rebeam.deviceToken";

export interface AuthUser { id: string; email: string; name: string; }
interface AuthResponse { token: string; user: AuthUser; }

// React development mode may initialize the app twice. Keep anonymous
// workspace creation single-flight so both callers receive the same device
// identity instead of creating duplicate empty workspaces.
let pendingBootstrap: Promise<AuthUser> | null = null;

export function sessionToken() { return localStorage.getItem(TOKEN_KEY); }

async function saveToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
  try { await invoke("save_device_token", { token }); } catch { /* browser dev fallback */ }
}

export async function currentUser() {
  const token = await sessionToken();
  if (!token) return null;
  const res = await fetch(`${RELAY}/auth/me`, { headers: { authorization: `Bearer ${token}` } });
  return res.ok ? res.json() as Promise<AuthUser> : null;
}

export async function bootstrap() {
  if (!pendingBootstrap) {
    pendingBootstrap = (async () => {
      const res = await fetch(`${RELAY}/bootstrap`, { method: "POST" });
      if (!res.ok) throw new Error("Could not create local workspace.");
      const result = await res.json() as AuthResponse;
      await saveToken(result.token);
      return result.user;
    })();
  }
  try {
    return await pendingBootstrap;
  } finally {
    pendingBootstrap = null;
  }
}

export async function createPairing() {
  const token = sessionToken();
  const res = await fetch(`${RELAY}/pairings`, { method: "POST", headers: token ? { authorization: `Bearer ${token}` } : {} });
  if (!res.ok) throw new Error("Could not create pairing code.");
  return (await res.json() as { code: string }).code;
}

export async function machineCount() {
  const token = sessionToken();
  const res = await fetch(`${RELAY}/machines`, { headers: token ? { authorization: `Bearer ${token}` } : {} });
  return res.ok ? await res.json() as { count: number; online: number } : { count: 0, online: 0 };
}

export async function logout() { localStorage.removeItem(TOKEN_KEY); }
