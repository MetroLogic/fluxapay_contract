"use client";

import { createContext, useContext, useEffect, useState } from "react";
import { isAuthenticated } from "@/lib/auth";

type AuthState = { authenticated: boolean; refresh: () => void };
const AuthContext = createContext<AuthState>({ authenticated: false, refresh: () => {} });

/** Issue #1165: shared authentication state for the dashboard shell. */
export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [authenticated, setAuthenticated] = useState(false);
  const refresh = () => setAuthenticated(isAuthenticated());

  useEffect(refresh, []);

  return <AuthContext.Provider value={{ authenticated, refresh }}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthState {
  return useContext(AuthContext);
}
