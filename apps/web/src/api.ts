/**
 * Base URL for API requests.
 * @type {string}
 */
export const API_BASE_URL = import.meta.env.VITE_API_BASE_URL ?? "";

/**
 * Fetches data from the API.
 * @template T - The type of the response data.
 * @param {string} path - The endpoint path.
 * @returns {Promise<T>} - A promise that resolves to the response data.
 */
export const api = {
  async get<T>(path: string): Promise<T> {
    const response = await fetch(`${API_BASE_URL}${path}`);
    if (!response.ok) throw new Error(await response.text());
    return response.json() as Promise<T>;
  },

  /**
   * Sends a POST request to the API.
   * @template T - The type of the response data.
   * @param {string} path - The endpoint path.
   * @param {unknown} [body] - The request body (optional).
   * @returns {Promise<T>} - A promise that resolves to the response data.
   */
  async post<T>(path: string, body?: unknown): Promise<T> {
    const response = await fetch(`${API_BASE_URL}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!response.ok) throw new Error(await response.text());
    if (response.status === 204) return undefined as T;
    return response.json() as Promise<T>;
  },

  /**
   * Sends a PATCH request to the API.
   * @template T - The type of the response data.
   * @param {string} path - The endpoint path.
   * @param {unknown} body - The request body.
   * @returns {Promise<T>} - A promise that resolves to the response data.
   */
  async patch<T>(path: string, body: unknown): Promise<T> {
    const response = await fetch(`${API_BASE_URL}${path}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!response.ok) throw new Error(await response.text());
    return response.json() as Promise<T>;
  },

  /**
   * Sends a DELETE request to the API.
   * @param {string} path - The endpoint path.
   * @returns {Promise<void>} - A promise that resolves when the request is complete.
   */
  async delete(path: string): Promise<void> {
    const response = await fetch(`${API_BASE_URL}${path}`, { method: "DELETE" });
    if (!response.ok) throw new Error(await response.text());
  },
};
