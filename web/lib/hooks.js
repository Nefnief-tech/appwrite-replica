"use client";

import { useCallback, useEffect, useState } from "react";
import { api } from "./api";

export function useApi(path, options = {}) {
  const { enabled = true } = options;
  const [state, setState] = useState({
    data: null,
    error: null,
    loading: Boolean(enabled),
  });
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (!enabled || !path) {
      setState((current) => ({ ...current, loading: false }));
      return;
    }
    let cancelled = false;
    setState((current) => ({ ...current, loading: true, error: null }));
    api(path)
      .then((data) => {
        if (!cancelled) setState({ data, error: null, loading: false });
      })
      .catch((error) => {
        if (!cancelled) setState({ data: null, error, loading: false });
      });
    return () => {
      cancelled = true;
    };
  }, [path, enabled, tick]);

  const reload = useCallback(() => setTick((value) => value + 1), []);
  return { ...state, reload, setData: (data) => setState({ data, error: null, loading: false }) };
}

export function useToast() {
  const [toast, setToast] = useState(null);
  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(() => setToast(null), 4200);
    return () => clearTimeout(timer);
  }, [toast]);
  const notify = (message, tone = "") => setToast({ message, tone });
  return { toast, notify };
}
