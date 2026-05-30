"use client"

import { useState } from "react"
import { Search, Sparkles, ExternalLink } from "lucide-react"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"

interface Source {
  id: string
  title: string
  journal: string
  snippet: string
  year: number
  selected: boolean
}

const mockSources: Source[] = [
  {
    id: "1",
    title: "Optimal Control Theory in Epidemic Modeling",
    journal: "Journal of Mathematical Biology",
    snippet: "This paper presents a comprehensive framework for applying optimal control theory to compartmental epidemic models...",
    year: 2024,
    selected: false,
  },
  {
    id: "2",
    title: "Deep Learning for Differential Equations",
    journal: "Nature Computational Science",
    snippet: "We propose physics-informed neural networks that leverage automatic differentiation for solving nonlinear PDEs...",
    year: 2023,
    selected: true,
  },
  {
    id: "3",
    title: "Stochastic SIR Models with Vaccination",
    journal: "SIAM Journal on Applied Mathematics",
    snippet: "A rigorous analysis of stochastic perturbations in SIR vaccination models reveals bifurcation behavior...",
    year: 2024,
    selected: true,
  },
  {
    id: "4",
    title: "Parameter Estimation in Dynamic Systems",
    journal: "Automatica",
    snippet: "Novel methods for real-time parameter estimation using Kalman filtering and maximum likelihood approaches...",
    year: 2023,
    selected: false,
  },
]

const aiResponse = `Based on your query about **disease transmission modeling**, I've analyzed relevant literature and identified key methodological approaches:

**1. Compartmental Models**
The SIR (Susceptible-Infected-Recovered) framework remains foundational. Recent extensions include age-structured populations and spatial heterogeneity.

**2. Optimal Control Applications**
Vaccination and quarantine strategies can be optimized using Pontryagin's Maximum Principle. The Hamiltonian formulation allows for cost-benefit analysis.

**3. Data-Driven Approaches**
Physics-informed neural networks (PINNs) show promise for parameter estimation when dealing with incomplete data.

$$\\frac{dS}{dt} = -\\beta SI, \\quad \\frac{dI}{dt} = \\beta SI - \\gamma I$$`

function SourceCard({ source, onToggle }: { source: Source; onToggle: (id: string) => void }) {
  return (
    <div 
      className={cn(
        "relative p-4 rounded-lg border transition-all cursor-pointer group",
        "hover:border-primary/40 hover:shadow-[0_0_15px_-3px] hover:shadow-primary/20",
        source.selected 
          ? "border-primary/50 bg-primary/5 shadow-[0_0_15px_-3px] shadow-primary/20" 
          : "border-border bg-card"
      )}
      onClick={() => onToggle(source.id)}
    >
      <div className="absolute top-3 right-3">
        <Checkbox 
          checked={source.selected} 
          onCheckedChange={() => onToggle(source.id)}
          className="data-[state=checked]:bg-primary data-[state=checked]:border-primary"
        />
      </div>
      <div className="pr-8">
        <h4 className="font-medium text-sm text-foreground line-clamp-2 mb-1">
          {source.title}
        </h4>
        <div className="flex items-center gap-2 text-xs text-muted-foreground mb-2">
          <span className="text-primary/80">{source.journal}</span>
          <span>•</span>
          <span>{source.year}</span>
        </div>
        <p className="text-xs text-muted-foreground line-clamp-2">
          {source.snippet}
        </p>
      </div>
      <button className="absolute bottom-3 right-3 opacity-0 group-hover:opacity-100 transition-opacity">
        <ExternalLink className="w-3.5 h-3.5 text-muted-foreground hover:text-foreground" />
      </button>
    </div>
  )
}

function SearchSkeleton() {
  return (
    <div className="space-y-4">
      <div className="space-y-3">
        <Skeleton className="h-4 w-3/4 bg-muted" />
        <Skeleton className="h-4 w-full bg-muted" />
        <Skeleton className="h-4 w-5/6 bg-muted" />
      </div>
      <div className="space-y-3 pt-4">
        <Skeleton className="h-3 w-1/4 bg-muted" />
        <Skeleton className="h-4 w-full bg-muted" />
        <Skeleton className="h-4 w-4/5 bg-muted" />
      </div>
      <div className="grid grid-cols-2 gap-3 pt-4">
        {[1, 2, 3, 4].map((i) => (
          <div key={i} className="p-4 rounded-lg border border-border bg-card">
            <Skeleton className="h-4 w-3/4 bg-muted mb-2" />
            <Skeleton className="h-3 w-1/2 bg-muted mb-3" />
            <Skeleton className="h-3 w-full bg-muted" />
            <Skeleton className="h-3 w-4/5 bg-muted mt-1" />
          </div>
        ))}
      </div>
    </div>
  )
}

export function MainWorkspace() {
  const [query, setQuery] = useState("")
  const [sources, setSources] = useState(mockSources)
  const [isSearching, setIsSearching] = useState(false)
  const [hasSearched, setHasSearched] = useState(true)

  const handleSearch = () => {
    if (!query.trim()) return
    setIsSearching(true)
    setHasSearched(false)
    setTimeout(() => {
      setIsSearching(false)
      setHasSearched(true)
    }, 2000)
  }

  const toggleSource = (id: string) => {
    setSources(sources.map(s => 
      s.id === id ? { ...s, selected: !s.selected } : s
    ))
  }

  const selectedCount = sources.filter(s => s.selected).length

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Search Bar */}
      <div className="p-6 border-b border-border">
        <div className="relative max-w-2xl mx-auto">
          <Search className="absolute left-4 top-1/2 -translate-y-1/2 w-5 h-5 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSearch()}
            placeholder="What problem are we modeling today?"
            className="pl-12 pr-4 py-6 text-base bg-input border-border focus:border-primary/50 focus:ring-primary/20 rounded-xl"
          />
          <div className="absolute right-2 top-1/2 -translate-y-1/2">
            <Button 
              size="sm" 
              onClick={handleSearch}
              className="bg-primary text-primary-foreground hover:bg-primary/90"
            >
              <Sparkles className="w-4 h-4 mr-1.5" />
              Search
            </Button>
          </div>
        </div>
      </div>

      {/* Content Area */}
      <ScrollArea className="flex-1">
        <div className="p-6 max-w-4xl mx-auto">
          {isSearching ? (
            <SearchSkeleton />
          ) : hasSearched ? (
            <>
              {/* AI Response */}
              <div className="mb-8">
                <div className="flex items-center gap-2 mb-4">
                  <div className="w-6 h-6 rounded-md bg-primary/20 flex items-center justify-center">
                    <Sparkles className="w-4 h-4 text-primary" />
                  </div>
                  <span className="text-sm font-medium text-foreground">AI Analysis</span>
                </div>
                <div className="prose prose-invert prose-sm max-w-none text-muted-foreground">
                  {aiResponse.split('\n').map((line, i) => {
                    if (line.startsWith('$$')) {
                      return (
                        <div key={i} className="my-4 p-4 bg-muted rounded-lg font-mono text-sm text-foreground overflow-x-auto">
                          {line.replace(/\$/g, '')}
                        </div>
                      )
                    }
                    if (line.startsWith('**') && line.endsWith('**')) {
                      return <h4 key={i} className="text-foreground font-semibold mt-4 mb-2">{line.replace(/\*\*/g, '')}</h4>
                    }
                    if (line.includes('**')) {
                      const parts = line.split(/\*\*(.*?)\*\*/)
                      return (
                        <p key={i} className="mb-2">
                          {parts.map((part, j) => 
                            j % 2 === 1 ? <strong key={j} className="text-foreground">{part}</strong> : part
                          )}
                        </p>
                      )
                    }
                    return line ? <p key={i} className="mb-2">{line}</p> : null
                  })}
                </div>
              </div>

              {/* Retrieved Sources */}
              <div>
                <div className="flex items-center justify-between mb-4">
                  <h3 className="text-sm font-medium text-foreground">Retrieved Sources</h3>
                  <span className="text-xs text-muted-foreground">{selectedCount} selected</span>
                </div>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                  {sources.map((source) => (
                    <SourceCard 
                      key={source.id} 
                      source={source} 
                      onToggle={toggleSource}
                    />
                  ))}
                </div>
              </div>
            </>
          ) : (
            <div className="flex flex-col items-center justify-center py-20 text-center">
              <div className="w-16 h-16 rounded-2xl bg-primary/10 flex items-center justify-center mb-4">
                <Sparkles className="w-8 h-8 text-primary" />
              </div>
              <h3 className="text-lg font-medium text-foreground mb-2">Start Your Research</h3>
              <p className="text-sm text-muted-foreground max-w-sm">
                Ask about mathematical models, differential equations, optimization problems, or any scientific modeling challenge.
              </p>
            </div>
          )}
        </div>
      </ScrollArea>

      {/* Sticky Action Button */}
      {selectedCount > 0 && (
        <div className="sticky bottom-0 p-4 border-t border-border bg-background/80 backdrop-blur-sm">
          <Button 
            className="w-full py-6 text-base font-medium bg-primary text-primary-foreground hover:bg-primary/90 shadow-[0_0_30px_-5px] shadow-primary/50 transition-all hover:shadow-[0_0_40px_-5px] hover:shadow-primary/60"
          >
            <Sparkles className="w-5 h-5 mr-2" />
            Feed {selectedCount} Selected Source{selectedCount > 1 ? 's' : ''} into Modeling Agent
          </Button>
        </div>
      )}
    </div>
  )
}
