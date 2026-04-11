import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { Sidebar } from "../components/layout/Sidebar";
import { usePlatformStore } from "../stores/platformStore";

// Mock the platformStore to avoid real Tauri invocations
vi.mock("../stores/platformStore", () => ({
  usePlatformStore: vi.fn(),
}));

// Mock the collectionStore
vi.mock("../stores/collectionStore", () => ({
  useCollectionStore: vi.fn(),
}));

import { useCollectionStore } from "../stores/collectionStore";

const mockAgents = [
  {
    id: "claude-code",
    display_name: "Claude Code",
    category: "coding",
    global_skills_dir: "~/.claude/skills/",
    is_detected: true,
    is_builtin: true,
    is_enabled: true,
  },
  {
    id: "cursor",
    display_name: "Cursor",
    category: "coding",
    global_skills_dir: "~/.cursor/skills/",
    is_detected: true,
    is_builtin: true,
    is_enabled: true,
  },
  {
    id: "central",
    display_name: "Central Skills",
    category: "central",
    global_skills_dir: "~/.agents/skills/",
    is_detected: true,
    is_builtin: true,
    is_enabled: true,
  },
];

const defaultStoreState = {
  agents: mockAgents,
  skillsByAgent: {
    "claude-code": 5,
    cursor: 3,
    central: 10,
  },
  isLoading: false,
  error: null,
  initialize: vi.fn(),
  rescan: vi.fn(),
};

const defaultCollectionState = {
  collections: [],
  currentDetail: null,
  isLoading: false,
  isLoadingDetail: false,
  error: null,
  loadCollections: vi.fn(),
  createCollection: vi.fn(),
  updateCollection: vi.fn(),
  deleteCollection: vi.fn(),
  loadCollectionDetail: vi.fn(),
  addSkillToCollection: vi.fn(),
  removeSkillFromCollection: vi.fn(),
  batchInstallCollection: vi.fn(),
  exportCollection: vi.fn(),
  importCollection: vi.fn(),
};

function renderSidebar(initialPath = "/central") {
  vi.mocked(usePlatformStore).mockReturnValue(defaultStoreState);
  return render(
    <MemoryRouter initialEntries={[initialPath]}>
      <Sidebar />
    </MemoryRouter>
  );
}

describe("Sidebar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Default: collection store returns empty state.
    vi.mocked(useCollectionStore).mockImplementation((selector) =>
      selector(defaultCollectionState)
    );
  });

  // ── Rendering ─────────────────────────────────────────────────────────────

  it("renders the app title", () => {
    renderSidebar();
    expect(screen.getByText("skills-manage")).toBeInTheDocument();
  });

  it("renders By Tool section header", () => {
    renderSidebar();
    expect(screen.getByText("按工具")).toBeInTheDocument();
  });

  it("renders platform agents in By Tool section", () => {
    renderSidebar();
    // Should show platform agents (not the central one)
    expect(screen.getByText("Claude Code")).toBeInTheDocument();
    expect(screen.getByText("Cursor")).toBeInTheDocument();
    // Central should not appear in By Tool
    // (it's rendered as a separate "Central Skills" nav item, not in By Tool)
  });

  it("shows skill count badges for each platform", () => {
    renderSidebar();
    // Claude Code has 5, Cursor has 3
    expect(screen.getByText("5")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("renders Central Skills nav item", () => {
    renderSidebar();
    expect(screen.getByText("中央技能库")).toBeInTheDocument();
  });

  it("shows Central Skills count badge", () => {
    renderSidebar();
    expect(screen.getByText("10")).toBeInTheDocument();
  });

  it("renders Collections section", () => {
    renderSidebar();
    expect(screen.getByText("技能集")).toBeInTheDocument();
  });

  it("renders '+新建' button in Collections section", () => {
    renderSidebar();
    expect(screen.getByText("+ 新建")).toBeInTheDocument();
  });

  it("renders Settings link at bottom", () => {
    renderSidebar();
    expect(screen.getByText("设置")).toBeInTheDocument();
  });

  // ── Loading State ─────────────────────────────────────────────────────────

  it("shows loading indicator when isLoading is true", () => {
    vi.mocked(usePlatformStore).mockReturnValue({
      ...defaultStoreState,
      isLoading: true,
    });
    render(
      <MemoryRouter>
        <Sidebar />
      </MemoryRouter>
    );
    expect(screen.getByText("扫描中...")).toBeInTheDocument();
  });

  it("hides platform list when loading", () => {
    vi.mocked(usePlatformStore).mockReturnValue({
      ...defaultStoreState,
      isLoading: true,
    });
    render(
      <MemoryRouter>
        <Sidebar />
      </MemoryRouter>
    );
    expect(screen.queryByText("Claude Code")).not.toBeInTheDocument();
  });

  // ── Active Route Highlighting ─────────────────────────────────────────────

  it("highlights active platform route in sidebar", () => {
    renderSidebar("/platform/claude-code");
    const claudeButton = screen.getByRole("button", { name: /Claude Code/ });
    // Active item should have the active class applied
    expect(claudeButton.className).toContain("font-medium");
  });

  it("highlights Central Skills when on /central", () => {
    renderSidebar("/central");
    const centralButton = screen.getByRole("button", { name: /中央技能库/ });
    expect(centralButton.className).toContain("font-medium");
  });

  it("highlights Settings when on /settings", () => {
    renderSidebar("/settings");
    const settingsButton = screen.getByRole("button", { name: /设置/ });
    expect(settingsButton.className).toContain("font-medium");
  });

  // ── Empty States ──────────────────────────────────────────────────────────

  it("shows empty message when no platforms are detected", () => {
    vi.mocked(usePlatformStore).mockReturnValue({
      ...defaultStoreState,
      agents: [
        {
          id: "central",
          display_name: "Central Skills",
          category: "central",
          global_skills_dir: "~/.agents/skills/",
          is_detected: true,
          is_builtin: true,
          is_enabled: true,
        },
      ],
    });
    render(
      <MemoryRouter>
        <Sidebar />
      </MemoryRouter>
    );
    expect(screen.getByText("没有检测到平台")).toBeInTheDocument();
  });

  // ── Navigation ────────────────────────────────────────────────────────────

  it("platform buttons are clickable", () => {
    renderSidebar();
    const claudeButton = screen.getByRole("button", { name: /Claude Code/ });
    expect(claudeButton).not.toBeDisabled();
    // Just verify it can be clicked without throwing
    fireEvent.click(claudeButton);
  });

  it("Central Skills button is clickable", () => {
    renderSidebar();
    const centralButton = screen.getByRole("button", { name: /中央技能库/ });
    expect(centralButton).not.toBeDisabled();
    fireEvent.click(centralButton);
  });

  // ── Collections ───────────────────────────────────────────────────────────

  it("shows collection names when collections are loaded", () => {
    vi.mocked(useCollectionStore).mockImplementation((selector) =>
      selector({
        ...defaultCollectionState,
        collections: [
          { id: "col-1", name: "Frontend", created_at: "2026-04-09T00:00:00Z", updated_at: "2026-04-09T00:00:00Z" },
          { id: "col-2", name: "Backend", created_at: "2026-04-09T00:00:00Z", updated_at: "2026-04-09T00:00:00Z" },
        ],
      })
    );
    renderSidebar();
    expect(screen.getByRole("button", { name: "Frontend" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Backend" })).toBeInTheDocument();
  });

  it("highlights active collection route", () => {
    vi.mocked(useCollectionStore).mockImplementation((selector) =>
      selector({
        ...defaultCollectionState,
        collections: [
          { id: "col-1", name: "Frontend", created_at: "2026-04-09T00:00:00Z", updated_at: "2026-04-09T00:00:00Z" },
        ],
      })
    );
    vi.mocked(usePlatformStore).mockReturnValue(defaultStoreState);
    render(
      <MemoryRouter initialEntries={["/collection/col-1"]}>
        <Sidebar />
      </MemoryRouter>
    );
    const colButton = screen.getByRole("button", { name: "Frontend" });
    expect(colButton.className).toContain("font-medium");
  });

  it("renders import button in Collections section", () => {
    renderSidebar();
    expect(screen.getByRole("button", { name: /导入技能集/i })).toBeInTheDocument();
  });

  // ── Collapse Toggle ───────────────────────────────────────────────────────

  it("renders a collapse toggle button", () => {
    renderSidebar();
    expect(screen.getByRole("button", { name: /折叠侧边栏/i })).toBeInTheDocument();
  });

  it("collapses sidebar when toggle button is clicked", () => {
    renderSidebar();
    const toggleBtn = screen.getByRole("button", { name: /折叠侧边栏/i });
    fireEvent.click(toggleBtn);
    // After collapse, the expand button should appear
    expect(screen.getByRole("button", { name: /展开侧边栏/i })).toBeInTheDocument();
  });

  it("expands sidebar when expand button is clicked after collapse", () => {
    renderSidebar();
    // Collapse
    fireEvent.click(screen.getByRole("button", { name: /折叠侧边栏/i }));
    // Expand again
    fireEvent.click(screen.getByRole("button", { name: /展开侧边栏/i }));
    // Should show collapse button again
    expect(screen.getByRole("button", { name: /折叠侧边栏/i })).toBeInTheDocument();
  });
});
