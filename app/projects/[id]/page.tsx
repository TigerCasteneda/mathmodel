import { ProjectPageClient } from "./client"

export async function generateStaticParams() {
  return [{ id: "_" }]
}

export default async function Page({
  params,
}: {
  params: Promise<{ id: string }>
}) {
  const { id } = await params
  return <ProjectPageClient id={id} />
}
