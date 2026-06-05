"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { getToken, getWebSocketBase } from "@/lib/api"

type SignalType =
  | "share_offer"
  | "share_answer"
  | "ice_candidate"
  | "share_started"
  | "share_stopped"

type ScreenSignal = {
  type: SignalType
  sender_id?: string
  target_user_id?: string
  payload?: any
}

function decodeUserIdFromToken(token: string | null): string | null {
  if (!token) return null
  try {
    const payload = token.split(".")[1]
    const normalized = payload.replace(/-/g, "+").replace(/_/g, "/")
    const decoded = JSON.parse(window.atob(normalized))
    return typeof decoded.sub === "string" ? decoded.sub : null
  } catch {
    return null
  }
}

function createPeerConnection(
  onIceCandidate: (candidate: RTCIceCandidate) => void,
  onRemoteStream?: (stream: MediaStream) => void,
) {
  const pc = new RTCPeerConnection({
    iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
  })
  pc.onicecandidate = (event) => {
    if (event.candidate) onIceCandidate(event.candidate)
  }
  if (onRemoteStream) {
    pc.ontrack = (event) => {
      const [stream] = event.streams
      if (stream) onRemoteStream(stream)
    }
  }
  return pc
}

export function useScreenShare(projectId: string, capabilities: string[]) {
  const canShare = capabilities.includes("screen.share")
  const canView = capabilities.includes("screen.view")
  const userIdRef = useRef<string | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const pcRef = useRef<RTCPeerConnection | null>(null)
  const sharingTargetRef = useRef<string | null>(null)
  const localStreamRef = useRef<MediaStream | null>(null)
  const sharingRef = useRef(false)
  const remoteSharingUserIdRef = useRef<string | null>(null)
  const [connected, setConnected] = useState(false)
  const [sharing, setSharing] = useState(false)
  const [remoteStream, setRemoteStream] = useState<MediaStream | null>(null)
  const [remoteSharingUserId, setRemoteSharingUserId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const sendSignal = useCallback((signal: ScreenSignal) => {
    const ws = wsRef.current
    if (ws?.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ payload: {}, ...signal }))
    }
  }, [])

  const closePeer = useCallback(() => {
    pcRef.current?.close()
    pcRef.current = null
    sharingTargetRef.current = null
  }, [])

  const stopShare = useCallback(() => {
    sendSignal({ type: "share_stopped" })
    localStreamRef.current?.getTracks().forEach((track) => track.stop())
    localStreamRef.current = null
    closePeer()
    sharingRef.current = false
    setSharing(false)
  }, [closePeer, sendSignal])

  const stopWatching = useCallback(() => {
    closePeer()
    setRemoteStream(null)
    remoteSharingUserIdRef.current = null
    setRemoteSharingUserId(null)
  }, [closePeer])

  const handleSignal = useCallback(async (signal: ScreenSignal) => {
    if (signal.sender_id && signal.sender_id === userIdRef.current) return

    if (signal.type === "share_started") {
      remoteSharingUserIdRef.current = signal.sender_id ?? null
      setRemoteSharingUserId(signal.sender_id ?? null)
      return
    }

    if (signal.type === "share_stopped") {
      if (!signal.sender_id || signal.sender_id === remoteSharingUserIdRef.current) stopWatching()
      return
    }

    if (signal.type === "share_offer" && canView && signal.sender_id) {
      closePeer()
      remoteSharingUserIdRef.current = signal.sender_id
      setRemoteSharingUserId(signal.sender_id)
      const pc = createPeerConnection(
        (candidate) => sendSignal({
          type: "ice_candidate",
          target_user_id: signal.sender_id,
          payload: { candidate: candidate.toJSON() },
        }),
        setRemoteStream,
      )
      pcRef.current = pc
      await pc.setRemoteDescription(signal.payload?.description)
      const answer = await pc.createAnswer()
      await pc.setLocalDescription(answer)
      sendSignal({
        type: "share_answer",
        target_user_id: signal.sender_id,
        payload: { description: answer },
      })
      return
    }

    if (signal.type === "share_answer" && sharingRef.current && signal.payload?.description) {
      await pcRef.current?.setRemoteDescription(signal.payload.description)
      return
    }

    if (signal.type === "ice_candidate" && signal.payload?.candidate) {
      await pcRef.current?.addIceCandidate(signal.payload.candidate).catch(() => {})
    }
  }, [canView, closePeer, sendSignal, stopWatching])

  useEffect(() => {
    if (!canShare && !canView) return
    let cancelled = false
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null

    const connect = async () => {
      const token = getToken()
      userIdRef.current = decodeUserIdFromToken(token)
      if (!token) return
      const base = await getWebSocketBase()
      if (cancelled) return
      const ws = new WebSocket(`${base}/projects/${encodeURIComponent(projectId)}/screen/ws?token=${encodeURIComponent(token)}`)
      wsRef.current = ws
      ws.onopen = () => setConnected(true)
      ws.onmessage = (event) => {
        try {
          void handleSignal(JSON.parse(event.data))
        } catch {
          /* ignore invalid signaling messages */
        }
      }
      ws.onclose = () => {
        setConnected(false)
        if (!cancelled) reconnectTimer = setTimeout(connect, 2000)
      }
      ws.onerror = () => setError("Screen signaling connection failed.")
    }

    void connect()
    return () => {
      cancelled = true
      if (reconnectTimer) clearTimeout(reconnectTimer)
      wsRef.current?.close()
      stopShare()
      stopWatching()
    }
  }, [canShare, canView, handleSignal, projectId, stopShare, stopWatching])

  const startShare = useCallback(async () => {
    if (!canShare) return
    setError(null)
    try {
      const stream = await navigator.mediaDevices.getDisplayMedia({
        video: true,
        audio: false,
      })
      localStreamRef.current = stream
      closePeer()
      const pc = createPeerConnection((candidate) => {
        sendSignal({
          type: "ice_candidate",
          target_user_id: sharingTargetRef.current ?? undefined,
          payload: { candidate: candidate.toJSON() },
        })
      })
      pcRef.current = pc
      stream.getTracks().forEach((track) => pc.addTrack(track, stream))
      stream.getVideoTracks()[0]?.addEventListener("ended", stopShare)
      const offer = await pc.createOffer()
      await pc.setLocalDescription(offer)
      sharingRef.current = true
      setSharing(true)
      sendSignal({ type: "share_started" })
      sendSignal({ type: "share_offer", payload: { description: offer } })
    } catch (err) {
      setError(err instanceof Error ? err.message : "Screen share failed.")
      stopShare()
    }
  }, [canShare, closePeer, sendSignal, stopShare])

  return {
    canShare,
    canView,
    connected,
    sharing,
    remoteStream,
    remoteSharingUserId,
    error,
    startShare,
    stopShare,
    stopWatching,
  }
}
