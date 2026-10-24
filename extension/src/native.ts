// A request/response client over a native messaging port. focus-nmh answers each message with
// the same `id`, so concurrent requests are matched up without relying on order.

import { PROTOCOL_VERSION, isIncoming, type Request, type Response } from "./protocol";

/** The parts of a runtime.Port this client uses, so tests can pass a fake. */
export interface PortLike {
  postMessage(message: unknown): void;
  onMessage: { addListener(listener: (message: unknown) => void): void };
  onDisconnect: { addListener(listener: () => void): void };
  disconnect(): void;
}

interface Pending {
  resolve(response: Response): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

/** Sends requests to the daemon through focus-nmh, reconnecting after the port drops. */
export class NativeClient {
  private port: PortLike | null = null;
  private nextId = 1;
  private readonly pending = new Map<number, Pending>();

  constructor(
    private readonly connect: () => PortLike,
    private readonly timeoutMs = 5000,
  ) {}

  /** Whether a port is currently open. */
  get connected(): boolean {
    return this.port !== null;
  }

  /**
   * Sends `request`. Resolves with the daemon's answer, error answers included; rejects only if
   * the host can't be reached, disconnects, or doesn't answer in time.
   */
  request(request: Request): Promise<Response> {
    const port = this.ensurePort();
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error("focus-nmh didn't answer in time"));
      }, this.timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      try {
        port.postMessage({ id, version: PROTOCOL_VERSION, message: request });
      } catch (error) {
        this.drop(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  private ensurePort(): PortLike {
    if (this.port) return this.port;
    const port = this.connect();
    port.onMessage.addListener((message) => this.receive(message));
    port.onDisconnect.addListener(() => {
      if (this.port === port) this.drop(new Error("focus-nmh disconnected"));
    });
    this.port = port;
    return port;
  }

  private receive(message: unknown): void {
    if (!isIncoming(message)) return;
    const pending = this.pending.get(message.id);
    if (!pending) return;
    clearTimeout(pending.timer);
    this.pending.delete(message.id);
    pending.resolve(message.message);
  }

  /** Forgets the port and fails every request still waiting on it. */
  private drop(error: Error): void {
    const port = this.port;
    this.port = null;
    for (const [id, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(error);
      this.pending.delete(id);
    }
    try {
      port?.disconnect();
    } catch {
      // Already closed.
    }
  }
}
