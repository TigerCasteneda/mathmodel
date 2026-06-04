"use client"

import { useParams } from "next/navigation"
import { ModelerWorkbench } from "@/components/layout/modeler-workbench"

export function ProjectPageClient({ id: propId }: { id?: string }) {
  const params = useParams<{ id: string }>()
  const id = propId || params.id

  return <ModelerWorkbench projectId={id} />
}
