"use client"

import { useEffect, useState } from "react"
import { useRouter } from "next/navigation"
import Link from "next/link"
import { Plus, LogOut, FolderGit2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { useAuth } from "@/hooks/use-auth"
import { apiFetch, joinProjectByCode, Project } from "@/lib/api"

export default function ProjectsPage() {
  const { user, loading, logout } = useAuth()
  const router = useRouter()
  const [projects, setProjects] = useState<Project[]>([])
  const [newName, setNewName] = useState("")
  const [inviteCode, setInviteCode] = useState("")
  const [creating, setCreating] = useState(false)
  const [joining, setJoining] = useState(false)
  const [fetching, setFetching] = useState(true)

  useEffect(() => {
    if (!loading && !user) {
      router.push("/login")
      return
    }
    if (user) {
      fetchProjects()
    }
  }, [user, loading])

  const fetchProjects = async () => {
    try {
      const data = await apiFetch<Project[]>("/projects")
      setProjects(data)
    } catch (err) {
      console.error("Failed to fetch projects", err)
    } finally {
      setFetching(false)
    }
  }

  const handleCreate = async () => {
    if (!newName.trim()) return
    setCreating(true)
    try {
      const project = await apiFetch<Project>("/projects", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: newName.trim() }),
      })
      setProjects((prev) => [project, ...prev])
      setNewName("")
    } catch (err) {
      console.error("Failed to create project", err)
    } finally {
      setCreating(false)
    }
  }

  const handleJoin = async () => {
    const code = inviteCode.trim()
    if (!code) return
    setJoining(true)
    try {
      const result = await joinProjectByCode(code)
      setInviteCode("")
      await fetchProjects()
      router.push(`/projects/${result.project_id}`)
    } catch (err) {
      console.error("Failed to join project", err)
    } finally {
      setJoining(false)
    }
  }

  const handleLogout = () => {
    logout()
    router.push("/login")
  }

  if (loading || fetching) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <p className="text-muted-foreground">Loading...</p>
      </div>
    )
  }

  return (
    <div className="min-h-screen bg-background">
      <div className="max-w-2xl mx-auto px-4 py-16">
        <div className="flex items-center justify-between mb-8">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-primary/20 flex items-center justify-center">
              <img src="/file-box.svg" alt="" className="h-5 w-5" />
            </div>
            <div>
              <h1 className="text-xl font-semibold text-foreground">Your Projects</h1>
              <p className="text-sm text-muted-foreground">
                {user?.display_name}
              </p>
            </div>
          </div>
          <Button variant="ghost" size="sm" onClick={handleLogout}>
            <LogOut className="w-4 h-4 mr-1.5" />
            Sign out
          </Button>
        </div>

        <div className="mb-8 space-y-3">
          <div className="flex gap-3">
            <Input
              placeholder="New project name..."
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleCreate()}
              className="bg-input border-border"
            />
            <Button
              onClick={handleCreate}
              disabled={creating || !newName.trim()}
              className="bg-primary text-primary-foreground hover:bg-primary/90 shrink-0"
            >
              <Plus className="w-4 h-4 mr-1.5" />
              Create
            </Button>
          </div>
          <div className="flex gap-3">
            <Input
              placeholder="Invite code..."
              value={inviteCode}
              onChange={(e) => setInviteCode(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleJoin()}
              className="bg-input border-border"
            />
            <Button
              onClick={handleJoin}
              disabled={joining || !inviteCode.trim()}
              variant="outline"
              className="shrink-0"
            >
              Join
            </Button>
          </div>
        </div>

        <div className="h-[60vh] overflow-y-auto">
          {projects.length === 0 ? (
            <div className="text-center py-16">
              <FolderGit2 className="w-12 h-12 text-muted-foreground/40 mx-auto mb-4" />
              <p className="text-muted-foreground">No projects yet. Create your first one above.</p>
            </div>
          ) : (
            <div className="space-y-2">
              {projects.map((p) => (
                <Link
                  key={p.id}
                  href={`/projects/${p.id}`}
                  className="block p-4 rounded-lg border border-border bg-card hover:border-primary/30 hover:bg-card/80 transition-all"
                >
                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="font-medium text-foreground">{p.name}</h3>
                      <p className="text-xs text-muted-foreground mt-0.5">
                        {p.role === "owner" ? "Owner" : p.role === "viewer" ? "Viewer" : "Editor"}{" "}
                        {new Date(p.updated_at * 1000).toLocaleDateString()}
                      </p>
                    </div>
                    <span className="text-xs px-2 py-1 rounded bg-primary/10 text-primary">
                      {p.role}
                    </span>
                  </div>
                </Link>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
