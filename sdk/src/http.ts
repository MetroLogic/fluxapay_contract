export interface RetryOptions {
  maxRetries?: number;
  logger?: (message: string) => void;
}

const DEFAULT_MAX_RETRIES = 3;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function isRetriableStatus(status: number): boolean {
  return status === 429 || status >= 500;
}

function getRetryDelayMs(res: Response, attempt: number): number {
  const retryAfter = res.headers.get('Retry-After');
  if (retryAfter) {
    const seconds = parseInt(retryAfter, 10);
    if (!Number.isNaN(seconds) && seconds >= 0) {
      return seconds * 1000;
    }
  }
  return Math.pow(2, attempt) * 1000;
}

/**
 * Performs a fetch with automatic retry and exponential backoff for
 * transient failures (429 Too Many Requests and 5xx server errors).
 *
 * Non-retriable responses (e.g. 400, 401, 403, 404, 422) are returned
 * immediately without retrying.
 */
export async function fetchWithRetry(
  url: string,
  options: RequestInit,
  maxRetries: number = DEFAULT_MAX_RETRIES,
  logger?: (message: string) => void,
): Promise<Response> {
  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    const res = await fetch(url, options);

    if (!isRetriableStatus(res.status)) {
      return res;
    }

    if (attempt === maxRetries) {
      return res;
    }

    const delay = getRetryDelayMs(res, attempt);
    if (logger) {
      logger(
        `[FluxaPay] Retry attempt ${attempt + 1}/${maxRetries} after ${delay}ms ` +
          `due to HTTP ${res.status}`,
      );
    }
    await sleep(delay);
  }

  // Unreachable: loop always returns on the final attempt.
  return fetch(url, options);
}
