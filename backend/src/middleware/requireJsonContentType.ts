import { Request, Response, NextFunction } from "express";

/**
 * Issue #838: Reject mutation requests that are not JSON.
 *
 * Browser CORS-exempt form POSTs use `application/x-www-form-urlencoded` (or
 * multipart) and would otherwise bypass preflight. Requiring
 * `Content-Type: application/json` for POST/PUT/PATCH closes that CSRF vector.
 */
export function requireJsonContentType(
  req: Request,
  res: Response,
  next: NextFunction,
): void {
  if (["POST", "PUT", "PATCH"].includes(req.method)) {
    if (!req.is("application/json")) {
      res.status(415).json({ error: "Unsupported Media Type" });
      return;
    }
  }
  next();
}
