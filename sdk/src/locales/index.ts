import en from "./en.json" with { type: "json" };
import fr from "./fr.json" with { type: "json" };
import pt from "./pt.json" with { type: "json" };
import es from "./es.json" with { type: "json" };

export type SupportedLocale = "en" | "fr" | "pt" | "es";

export const SUPPORTED_LOCALES: SupportedLocale[] = ["en", "fr", "pt", "es"];

export const MESSAGES: Record<string, Record<string, string>> = {
  en,
  fr,
  pt,
  es,
};

export function getLocalizedErrorMessage(
  code: number,
  locale = "en",
  defaultMessage?: string,
): string {
  const normalizedLocale = (locale || "en").toLowerCase();
  const selectedLocale = MESSAGES[normalizedLocale] ? normalizedLocale : "en";
  const messagesForLocale = MESSAGES[selectedLocale] || MESSAGES["en"];

  const localized = messagesForLocale?.[String(code)];
  if (localized) {
    return localized;
  }

  // Fallback to English if missing in chosen locale
  const fallback = MESSAGES["en"]?.[String(code)];
  if (fallback) {
    return fallback;
  }

  return defaultMessage ?? `Contract error #${code}`;
}
