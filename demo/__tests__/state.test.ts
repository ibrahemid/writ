import { describe, expect, it } from "vitest";
import { refuseOnThrow } from "../backend/state";

const describeAsWords = (error: unknown) => `refused: ${error instanceof Error ? error.message : String(error)}`;

describe("refuseOnThrow", () => {
  it("answers with the body's value when nothing is thrown", async () => {
    expect(refuseOnThrow(() => 7, describeAsWords)).toBe(7);
    await expect(refuseOnThrow(async () => 7, describeAsWords)).resolves.toBe(7);
  });

  it("rejects with the described string for a throw, now or later", async () => {
    await expect(
      refuseOnThrow(() => {
        throw new Error("now");
      }, describeAsWords),
    ).rejects.toBe("refused: now");
    await expect(refuseOnThrow(() => Promise.reject(new Error("later")), describeAsWords)).rejects.toBe(
      "refused: later",
    );
  });

  it("passes an error on when the describer throws it", async () => {
    const fault = new TypeError("a fault in the page");
    const rethrow = (error: unknown): string => {
      throw error;
    };
    expect(() =>
      refuseOnThrow(() => {
        throw fault;
      }, rethrow),
    ).toThrow(fault);
    await expect(refuseOnThrow(() => Promise.reject(fault), rethrow)).rejects.toBe(fault);
  });
});
