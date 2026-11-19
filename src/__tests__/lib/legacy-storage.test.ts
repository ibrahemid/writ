import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { adoptLegacyStorageKey } from "../../lib/legacy-storage";

// A value an earlier build kept under an old key moves to the current key once
// (ADR-059 decision 12).

const OLD = "old.key.v1";
const NEW = "new.key.v1";

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("adopting a value kept under an old key", () => {
  it("copies it to the current key and removes the old one", () => {
    localStorage.setItem(OLD, "kept");
    adoptLegacyStorageKey(OLD, NEW);
    expect(localStorage.getItem(NEW)).toBe("kept");
    expect(localStorage.getItem(OLD)).toBeNull();
  });

  it("leaves the current key's value when both are present, and removes the old one", () => {
    localStorage.setItem(OLD, "old");
    localStorage.setItem(NEW, "new");
    adoptLegacyStorageKey(OLD, NEW);
    expect(localStorage.getItem(NEW)).toBe("new");
    expect(localStorage.getItem(OLD)).toBeNull();
  });

  it("does nothing when only the current key is present", () => {
    localStorage.setItem(NEW, "new");
    adoptLegacyStorageKey(OLD, NEW);
    expect(localStorage.getItem(NEW)).toBe("new");
    expect(localStorage.length).toBe(1);
  });

  it("does nothing when neither key is present", () => {
    adoptLegacyStorageKey(OLD, NEW);
    expect(localStorage.length).toBe(0);
  });

  it("keeps the old key for the next launch when storage refuses the write", () => {
    localStorage.setItem(OLD, "kept");
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("quota", "QuotaExceededError");
    });
    expect(() => adoptLegacyStorageKey(OLD, NEW)).not.toThrow();
    vi.restoreAllMocks();
    expect(localStorage.getItem(OLD)).toBe("kept");
    expect(localStorage.getItem(NEW)).toBeNull();
  });

  it("does not throw when storage cannot be read", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("denied", "SecurityError");
    });
    expect(() => adoptLegacyStorageKey(OLD, NEW)).not.toThrow();
  });
});
