import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { FluxapayError, toFluxapayError, getLocalizedErrorMessage, MESSAGES, SUPPORTED_LOCALES } from "../src/index.js";

describe("SDK Error Localization (Issue #852)", () => {
  test("supported locales list contains initial supported locales", () => {
    assert.deepEqual(SUPPORTED_LOCALES, ["en", "fr", "pt", "es"]);
    assert.ok(MESSAGES["en"]);
    assert.ok(MESSAGES["fr"]);
    assert.ok(MESSAGES["pt"]);
    assert.ok(MESSAGES["es"]);
  });

  test("FluxapayError.localizedMessage returns English message by default", () => {
    const err = new FluxapayError(1, "Unauthorized", "Unauthorized operation", undefined, "en");
    assert.equal(err.locale, "en");
    assert.equal(err.localizedMessage, "Unauthorized operation");
  });

  test("FluxapayError.localizedMessage returns French message when locale is fr", () => {
    const err = new FluxapayError(1, "Unauthorized", "Unauthorized operation", undefined, "fr");
    assert.equal(err.locale, "fr");
    assert.equal(err.localizedMessage, "Opération non autorisée");

    const err404 = new FluxapayError(404, "PaymentNotFound", undefined, undefined, "fr");
    assert.equal(err404.localizedMessage, "Paiement introuvable");
  });

  test("FluxapayError.localizedMessage returns Portuguese and Spanish messages", () => {
    const errPt = new FluxapayError(1, "Unauthorized", undefined, undefined, "pt");
    assert.equal(errPt.localizedMessage, "Operação não autorizada");

    const errEs = new FluxapayError(1, "Unauthorized", undefined, undefined, "es");
    assert.equal(errEs.localizedMessage, "Operación no autorizada");
  });

  test("FluxapayError.localizedMessage falls back to English for unsupported locales", () => {
    const errUnsupported = new FluxapayError(2, "PaymentAlreadyExists", undefined, undefined, "de");
    assert.equal(errUnsupported.localizedMessage, "Payment already exists");

    const errJa = new FluxapayError(3, "PaymentExpired", undefined, undefined, "ja");
    assert.equal(errJa.localizedMessage, "Payment has expired");
  });

  test("toFluxapayError preserves and maps locale correctly", () => {
    const rawError = new Error("Error(Contract, #1)");
    const errFr = toFluxapayError(rawError, "fr");
    assert.equal(errFr.code, 1);
    assert.equal(errFr.locale, "fr");
    assert.equal(errFr.localizedMessage, "Opération non autorisée");

    const errFallback = toFluxapayError(rawError, "xx-unknown");
    assert.equal(errFallback.localizedMessage, "Unauthorized operation");
  });

  test("getLocalizedErrorMessage helper works directly with fallbacks", () => {
    assert.equal(getLocalizedErrorMessage(1, "fr"), "Opération non autorisée");
    assert.equal(getLocalizedErrorMessage(1, "en"), "Unauthorized operation");
    assert.equal(getLocalizedErrorMessage(1, "unknown"), "Unauthorized operation");
    // Unknown error code falls back to defaultMessage or placeholder
    assert.equal(getLocalizedErrorMessage(999999, "fr", "Default fallback"), "Default fallback");
  });
});
