"use client"

import { useEffect, useState } from "react"
import { toast } from "sonner"
import { Switch } from "./ui/switch"
import { AccelerationMode, AccelerationStatus, HardwareAccelerationAPI, crashedEngines } from "@/lib/hardwareAcceleration"

export function HardwareAccelerationSettings() {
  const [status, setStatus] = useState<AccelerationStatus | null>(null)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    HardwareAccelerationAPI.getStatus()
      .then(setStatus)
      .catch(error => console.error("Failed to load hardware acceleration status:", error))
  }, [])

  if (!status) return null

  const applyMode = async (mode: AccelerationMode) => {
    setSaving(true)
    try {
      await HardwareAccelerationAPI.setMode(mode)
      setStatus(await HardwareAccelerationAPI.getStatus())
      toast.success(mode === "auto" ? "GPU acceleration enabled" : "Running on the CPU only")
    } catch (error) {
      toast.error("Failed to change hardware acceleration", {
        description: error instanceof Error ? error.message : String(error),
      })
    } finally {
      setSaving(false)
    }
  }

  const crashed = crashedEngines(status)
  const hasGpuBackend = status.backend !== "Cpu"

  return (
    <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h3 className="text-lg font-semibold text-gray-900 mb-2">Use GPU acceleration</h3>
          <p className="text-sm text-gray-600">
            Speech recognition and built-in summaries run on the GPU when one is available, otherwise on the CPU.
          </p>
        </div>
        <Switch
          checked={status.mode === "auto"}
          disabled={saving || !hasGpuBackend}
          onCheckedChange={checked => applyMode(checked ? "auto" : "cpu")}
        />
      </div>

      <p className="mt-3 text-sm text-gray-700">
        {!hasGpuBackend
          ? "This build has no GPU support and always runs on the CPU."
          : status.gpu
            ? <>GPU: <strong>{status.gpu.description || status.gpu.name}</strong> ({status.backend}{status.gpu.integrated ? ", integrated" : ""})</>
            : `No ${status.backend} GPU found, running on the CPU.`}
      </p>

      {crashed && status.mode === "auto" && (
        <div className="mt-3 p-3 bg-amber-50 border border-amber-200 rounded text-amber-800 text-sm">
          <p>
            The GPU stopped responding during {crashed}, so TriNote switched {crashed} to the CPU.
            Updating your graphics driver may fix this.
          </p>
          <button
            onClick={() => applyMode("auto")}
            disabled={saving}
            className="mt-2 px-3 py-1.5 bg-amber-600 hover:bg-amber-700 text-white text-xs font-medium rounded disabled:opacity-50"
          >
            Try the GPU again
          </button>
        </div>
      )}
    </div>
  )
}

/** Tells the user once when a GPU crash made TriNote fall back to the CPU */
export function AccelerationCrashNotice() {
  useEffect(() => {
    HardwareAccelerationAPI.getStatus()
      .then(status => {
        const crashed = crashedEngines(status)
        if (!status.crash_notice_pending || !crashed) return
        toast.warning(`TriNote switched ${crashed} to the CPU`, {
          description: "The GPU stopped responding last time. You can turn it back on in Settings → General.",
          duration: 15000,
        })
        return HardwareAccelerationAPI.acknowledgeNotice()
      })
      .catch(error => console.error("Failed to check hardware acceleration status:", error))
  }, [])

  return null
}
