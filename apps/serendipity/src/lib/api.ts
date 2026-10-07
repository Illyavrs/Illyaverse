export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}
export async function api<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers);
  if (options.body && !(options.body instanceof FormData))
    headers.set('Content-Type', 'application/json');
  const response = await fetch(`/api/v1${path}`, { ...options, headers, credentials: 'include' });
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new ApiError(
      response.status,
      body?.error?.code ?? 'service_unavailable',
      body?.error?.message ?? `HTTP ${response.status}`,
    );
  }
  return response.status === 204 ? (undefined as T) : response.json();
}
export function hasPermission(permissions: string[], permission: string) {
  return permissions.includes('*') || permissions.includes(permission);
}
export const fetcher = <T>(path: string) => api<T>(path);
