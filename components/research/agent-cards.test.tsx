// Component test for `components/research/agent-cards.tsx` SourceCard.
// Requires vitest + @testing-library/react to run. If vitest is not
// configured at the project root, this file still validates the
// SourceCard rendering path against the expected DOM structure once
// the test runner is installed.
//
// Run with: `pnpm vitest run components/research/agent-cards.test.tsx`

import { describe, expect, it } from "vitest"
import { render } from "@testing-library/react"
import { SourceCard } from "./agent-cards"

describe("SourceCard", () => {
  it("renders structured_data as a table when present", () => {
    const source = {
      citation: 1,
      title: "Test Paper",
      url: "https://example.com",
      content: "snippet",
      provider: "scrapling",
      category: "literature",
      structured_data: { authors: "Alice, Bob", year: 2024 },
    }
    const { container } = render(
      <SourceCard source={source} selected={false} onToggle={() => {}} />,
    )
    const details = container.querySelector("details")
    expect(details).not.toBeNull()
    const rows = container.querySelectorAll("tbody tr")
    expect(rows.length).toBe(2)
  })

  it("omits structured_data block when null", () => {
    const source = {
      citation: 1,
      title: "Test",
      url: "https://example.com",
      content: "x",
      provider: "scrapling",
      category: "literature",
      structured_data: null,
    }
    const { container } = render(
      <SourceCard source={source} selected={false} onToggle={() => {}} />,
    )
    expect(container.querySelector("details")).toBeNull()
  })
})