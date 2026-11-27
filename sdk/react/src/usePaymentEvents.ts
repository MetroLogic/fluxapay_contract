import * as React from "react";

export interface PaymentEvent {
  event_type: string;
  merchant_id: string;
  payment_id?: string;
  timestamp: number;
  data?: any;
}

export type ConnectionStatus = "connecting" | "connected" | "disconnected" | "error";

export interface UsePaymentEventsOptions {
  baseUrl?: string;
  merchantId?: string;
  eventTypes?: string[];
  token?: string;
  onEvent?: (event: PaymentEvent) => void;
  enabled?: boolean;
}

export interface UsePaymentEventsResult {
  events: PaymentEvent[];
  latestEvent: PaymentEvent | null;
  status: ConnectionStatus;
  error: Error | null;
  close: () => void;
}

/**
 * React hook to subscribe to the real-time Server-Sent Events (SSE) payment stream.
 * 
 * Supports automatic connection management, keepalive handling, and event filtering.
 */
export function usePaymentEvents(options: UsePaymentEventsOptions = {}): UsePaymentEventsResult {
  const {
    baseUrl = "http://localhost:3000",
    merchantId,
    eventTypes,
    token,
    onEvent,
    enabled = true,
  } = options;

  const [events, setEvents] = React.useState<PaymentEvent[]>([]);
  const [latestEvent, setLatestEvent] = React.useState<PaymentEvent | null>(null);
  const [status, setStatus] = React.useState<ConnectionStatus>("disconnected");
  const [error, setError] = React.useState<Error | null>(null);

  const eventSourceRef = React.useRef<EventSource | null>(null);
  const onEventRef = React.useRef(onEvent);
  onEventRef.current = onEvent;

  const close = React.useCallback(() => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
      setStatus("disconnected");
    }
  }, []);

  React.useEffect(() => {
    if (!enabled || typeof window === "undefined" || !("EventSource" in window)) {
      return;
    }

    const queryParams = new URLSearchParams();
    if (merchantId) queryParams.set("merchant_id", merchantId);
    if (eventTypes && eventTypes.length > 0) queryParams.set("event_types", eventTypes.join(","));
    if (token) queryParams.set("token", token);

    const qs = queryParams.toString();
    const url = `${baseUrl.replace(/\/+$/, "")}/v1/events/stream${qs ? `?${qs}` : ""}`;

    setStatus("connecting");
    setError(null);

    const es = new EventSource(url);
    eventSourceRef.current = es;

    es.onopen = () => {
      setStatus("connected");
      setError(null);
    };

    es.onerror = () => {
      setStatus("error");
      setError(new Error("SSE connection error"));
    };

    const handleMessage = (e: MessageEvent) => {
      try {
        const parsed: PaymentEvent = JSON.parse(e.data);
        setLatestEvent(parsed);
        setEvents((prev) => [...prev, parsed]);
        if (onEventRef.current) {
          onEventRef.current(parsed);
        }
      } catch {
        // Ping or non-JSON event ignored
      }
    };

    es.onmessage = handleMessage;

    if (eventTypes && eventTypes.length > 0) {
      for (const evtType of eventTypes) {
        es.addEventListener(evtType, ((e: Event) => handleMessage(e as MessageEvent)) as EventListener);
      }
    }

    return () => {
      es.close();
      if (eventSourceRef.current === es) {
        eventSourceRef.current = null;
      }
      setStatus("disconnected");
    };
  }, [baseUrl, merchantId, JSON.stringify(eventTypes), token, enabled]);

  return { events, latestEvent, status, error, close };
}
