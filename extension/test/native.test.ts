import { afterEach, describe, expect, it, vi } from "vitest";

import { NativeClient, type PortLike } from "../src/native";
import type { Outgoing, Response } from "../src/protocol";

/** A port the test drives by hand. */
class FakePort implements PortLike {
  sent: Outgoing[] = [];
  disconnected = false;
  private messageListeners: ((message: unknown) => void)[] = [];
  private disconnectListeners: (() => void)[] = [];
  onMessage = { addListener: (listener: (message: unknown) => void) => void this.messageListeners.push(listener) };
  onDisconnect = { addListener: (listener: () => void) => void this.disconnectListeners.push(listener) };

  postMessage(message: unknown): void {
    this.sent.push(message as Outgoing);
  }
  disconnect(): void {
    this.disconnected = true;
  }
  answer(id: number, message: Response): void {
    for (const listener of this.messageListeners) listener({ id, version: 1, message });
  }
  hostExits(): void {
    for (const listener of this.disconnectListeners) listener();
  }
}

function clientWithPorts(timeoutMs = 5000) {
  const ports: FakePort[] = [];
  const client = new NativeClient(() => {
    const port = new FakePort();
    ports.push(port);
    return port;
  }, timeoutMs);
  return { client, ports };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("NativeClient", () => {
  it("matches answers to requests by id, in any order", async () => {
    const { client, ports } = clientWithPorts();
    const status = client.request({ type: "status" });
    const rules = client.request({ type: "rules_for", url: "https://x.com", config: null });
    const port = ports[0]!;
    expect(port.sent.map(({ id, version }) => [id, version])).toEqual([
      [1, 1],
      [2, 1],
    ]);
    port.answer(2, { type: "verdict", verdict: { restrict: "block" }, config: "coding" });
    port.answer(1, { type: "done" });
    await expect(rules).resolves.toEqual({ type: "verdict", verdict: { restrict: "block" }, config: "coding" });
    await expect(status).resolves.toEqual({ type: "done" });
    expect(ports).toHaveLength(1);
  });

  it("ignores messages it isn't waiting for", async () => {
    const { client, ports } = clientWithPorts();
    const request = client.request({ type: "status" });
    const port = ports[0]!;
    port.answer(99, { type: "done" });
    port.onMessage.addListener(() => undefined);
    for (const junk of [null, "text", { id: "1" }]) {
      (port as unknown as { messageListeners: ((m: unknown) => void)[] }).messageListeners[0]!(junk);
    }
    port.answer(1, { type: "done" });
    await expect(request).resolves.toEqual({ type: "done" });
  });

  it("fails waiting requests when the host exits, then reconnects", async () => {
    const { client, ports } = clientWithPorts();
    const waiting = client.request({ type: "status" });
    ports[0]!.hostExits();
    await expect(waiting).rejects.toThrow("disconnected");
    expect(client.connected).toBe(false);

    const next = client.request({ type: "status" });
    expect(ports).toHaveLength(2);
    ports[1]!.answer(2, { type: "done" });
    await expect(next).resolves.toEqual({ type: "done" });
  });

  it("gives up on a request that isn't answered in time", async () => {
    vi.useFakeTimers();
    const { client } = clientWithPorts(1000);
    const request = client.request({ type: "status" });
    vi.advanceTimersByTime(1001);
    await expect(request).rejects.toThrow("in time");
  });
});
