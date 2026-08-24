import {
  useState,
  useMemo,
  useEffect,
  forwardRef,
  useImperativeHandle,
} from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  RefreshCw,
  Search,
  Loader2,
  Settings,
  type LucideIcon,
} from "lucide-react";
import { toast } from "sonner";
import { SkillCard } from "./SkillCard";
import { RepoManagerPanel } from "./RepoManagerPanel";
import {
  useDiscoverableSkills,
  useLibrarySkills,
  useAcquireLibrarySkill,
  useSkillRepos,
  useAddSkillRepo,
  useRemoveSkillRepo,
  useSearchSkillsSh,
} from "@/hooks/useSkills";
import type {
  DiscoverableSkill,
  RemoteLibrarySourceKind,
  SkillRepo,
  SkillsShDiscoverableSkill,
} from "@/lib/api/skills";
import { formatSkillError } from "@/lib/errors/skillErrorParser";
import {
  showSkillErrorToast,
  SkillTechnicalDetails,
} from "@/components/skills/SkillTechnicalDetails";

export type SkillsPageSource = "repos" | "skillssh";

interface SkillsPageProps {
  onSourceChange?: (source: SkillsPageSource) => void;
}

export interface SkillsPageHandle {
  refresh: () => void;
  openRepoManager: () => void;
}

type SkillsPageHeaderAction = {
  key: string;
  sources: readonly SkillsPageSource[];
  labelKey: string;
  Icon: LucideIcon;
  execute: (page: SkillsPageHandle | null) => void;
};

const SKILLS_PAGE_HEADER_ACTIONS: readonly SkillsPageHeaderAction[] = [
  {
    key: "refresh-repos",
    sources: ["repos"],
    labelKey: "skills.refresh",
    Icon: RefreshCw,
    execute: (page) => page?.refresh(),
  },
  {
    key: "manage-repos",
    sources: ["repos", "skillssh"],
    labelKey: "skills.repoManager",
    Icon: Settings,
    execute: (page) => page?.openRepoManager(),
  },
];

export const getSkillsPageHeaderActions = (source: SkillsPageSource) =>
  SKILLS_PAGE_HEADER_ACTIONS.filter((action) =>
    action.sources.includes(source),
  );

const SKILLSSH_PAGE_SIZE = 20;

/**
 * Skills 发现面板
 * 用于浏览并获取来自仓库或 skills.sh 的 Skills
 */
export const SkillsPage = forwardRef<SkillsPageHandle, SkillsPageProps>(
  ({ onSourceChange }, ref) => {
    const { t } = useTranslation();
    const [repoManagerOpen, setRepoManagerOpen] = useState(false);
    const [searchQuery, setSearchQuery] = useState("");
    const [filterRepo, setFilterRepo] = useState<string>("all");
    const [filterStatus, setFilterStatus] = useState<
      "all" | "acquired" | "available"
    >("all");

    // skills.sh 搜索状态
    const [searchSource, setSearchSource] = useState<SkillsPageSource>("repos");
    const [skillsShInput, setSkillsShInput] = useState("");
    const [skillsShQuery, setSkillsShQuery] = useState("");
    const [skillsShOffset, setSkillsShOffset] = useState(0);
    const [accumulatedResults, setAccumulatedResults] = useState<
      SkillsShDiscoverableSkill[]
    >([]);

    // Queries
    const {
      data: discoverableSkills,
      isLoading: loadingDiscoverable,
      isFetching: fetchingDiscoverable,
      refetch: refetchDiscoverable,
    } = useDiscoverableSkills();
    const { data: librarySkills } = useLibrarySkills();
    const { data: repos = [], refetch: refetchRepos } = useSkillRepos();

    // skills.sh 搜索
    const {
      data: skillsShResult,
      isLoading: loadingSkillsSh,
      isFetching: fetchingSkillsSh,
      isPlaceholderData: placeholderSkillsSh,
    } = useSearchSkillsSh(skillsShQuery, SKILLSSH_PAGE_SIZE, skillsShOffset);

    // 当搜索结果返回时累积
    useEffect(() => {
      if (skillsShResult && !placeholderSkillsSh) {
        if (skillsShOffset === 0) {
          setAccumulatedResults(skillsShResult.skills);
        } else {
          setAccumulatedResults((prev) => [...prev, ...skillsShResult.skills]);
        }
      }
    }, [skillsShResult, skillsShOffset, placeholderSkillsSh]);

    // 手动提交搜索
    const handleSkillsShSearch = () => {
      const trimmed = skillsShInput.trim();
      if (trimmed.length < 2) return;
      if (trimmed === skillsShQuery && skillsShOffset === 0) return;
      setSkillsShOffset(0);
      setAccumulatedResults([]);
      setSkillsShQuery(trimmed);
    };

    // Mutations
    const acquireMutation = useAcquireLibrarySkill();
    const addRepoMutation = useAddSkillRepo();
    const removeRepoMutation = useRemoveSkillRepo();

    const acquiredKeys = useMemo(() => {
      if (!librarySkills) return new Set<string>();
      const keys = new Set<string>();
      for (const skill of librarySkills) {
        const owner = skill.source.repoOwner?.toLowerCase() || "";
        const repo = skill.source.repoName?.toLowerCase() || "";
        const sourcePath = skill.source.skillPath?.toLowerCase();
        if (sourcePath) {
          if (sourcePath === ".") keys.add(`*:${owner}:${repo}`);
          keys.add(`${sourcePath}:${owner}:${repo}`);
          const sourceName = sourcePath.split(/[/\\]/).pop();
          if (sourceName) keys.add(`${sourceName}:${owner}:${repo}`);
        }
        keys.add(`${skill.directory.toLowerCase()}:${owner}:${repo}`);
      }
      return keys;
    }, [librarySkills]);

    type DiscoverableSkillItem = DiscoverableSkill & { acquired: boolean };

    // 从可发现技能中提取所有仓库选项
    const repoOptions = useMemo(() => {
      if (!discoverableSkills) return [];
      const repoSet = new Set<string>();
      discoverableSkills.forEach((s) => {
        if (s.repoOwner && s.repoName) {
          repoSet.add(`${s.repoOwner}/${s.repoName}`);
        }
      });
      return Array.from(repoSet).sort();
    }, [discoverableSkills]);

    // 为发现列表补齐已获取状态，供 SkillCard 的兼容接口使用
    const skills: DiscoverableSkillItem[] = useMemo(() => {
      if (!discoverableSkills) return [];
      return discoverableSkills.map((d) => {
        // 同时处理 / 和 \ 路径分隔符（兼容 Windows 和 Unix）
        const sourceName =
          d.directory.split(/[/\\]/).pop()?.toLowerCase() ||
          d.directory.toLowerCase();
        // 使用 directory + repoOwner + repoName 组合判断是否已获取
        const key = `${sourceName}:${d.repoOwner.toLowerCase()}:${d.repoName.toLowerCase()}`;
        return {
          ...d,
          acquired:
            acquiredKeys.has(key) ||
            acquiredKeys.has(
              `*:${d.repoOwner.toLowerCase()}:${d.repoName.toLowerCase()}`,
            ) ||
            acquiredKeys.has(
              `${d.directory.toLowerCase()}:${d.repoOwner.toLowerCase()}:${d.repoName.toLowerCase()}`,
            ),
        };
      });
    }, [discoverableSkills, acquiredKeys]);

    // 检查 skills.sh 结果的获取状态
    const isSkillsShAcquired = (skill: SkillsShDiscoverableSkill): boolean => {
      const key = `${skill.directory.toLowerCase()}:${skill.repoOwner.toLowerCase()}:${skill.repoName.toLowerCase()}`;
      return acquiredKeys.has(key);
    };

    const loading =
      searchSource === "repos"
        ? loadingDiscoverable || fetchingDiscoverable
        : false;

    // With no configured repository, skills.sh is the real source even before
    // the user explicitly changes the source toggle. All actions must use the
    // same effective value that the UI renders.
    const effectiveSource =
      searchSource === "repos" && repos.length === 0 && !loading
        ? "skillssh"
        : searchSource;

    useImperativeHandle(ref, () => ({
      refresh: () => {
        refetchDiscoverable();
        refetchRepos();
      },
      openRepoManager: () => setRepoManagerOpen(true),
    }));

    // skills.sh 结果转为 DiscoverableSkill（复用统一获取流程）
    const toDiscoverableSkill = (
      s: SkillsShDiscoverableSkill,
    ): DiscoverableSkill => ({
      key: s.key,
      name: s.name,
      description: "",
      directory: s.directory,
      repoOwner: s.repoOwner,
      repoName: s.repoName,
      repoBranch: s.repoBranch,
      readmeUrl: s.readmeUrl,
    });

    const [collision, setCollision] = useState<{
      skill: DiscoverableSkill;
      sourceKind: RemoteLibrarySourceKind;
    } | null>(null);
    const [uniqueDirectory, setUniqueDirectory] = useState("");

    const acquireSkill = async (
      skill: DiscoverableSkill,
      sourceKind: RemoteLibrarySourceKind,
      directoryName?: string,
    ) => {
      await acquireMutation.mutateAsync({ skill, sourceKind, directoryName });
      toast.success(t("skills.library.acquireSuccess", { name: skill.name }), {
        closeButton: true,
      });
    };

    const handleAcquire = async (key: string) => {
      let skill: DiscoverableSkill | undefined;

      if (effectiveSource === "skillssh") {
        const found = accumulatedResults.find((s) => s.key === key);
        if (found) {
          skill = toDiscoverableSkill(found);
        }
      } else {
        skill = discoverableSkills?.find((s) => s.key === key);
      }

      if (!skill) {
        toast.error(t("skills.notFound"));
        return;
      }

      try {
        const sourceKind =
          effectiveSource === "skillssh" ? "marketplace" : "git";
        await acquireSkill(skill, sourceKind);
      } catch (error) {
        const errorMessage =
          error instanceof Error ? error.message : String(error);
        if (errorMessage.includes("LIBRARY_DIRECTORY_CONFLICT")) {
          setCollision({
            skill,
            sourceKind: effectiveSource === "skillssh" ? "marketplace" : "git",
          });
          setUniqueDirectory(`${skill.directory.split(/[/\\]/).pop()}-2`);
          return;
        }
        const { title, description, technicalDetails } = formatSkillError(
          errorMessage,
          t,
          "skills.library.acquireFailed",
        );
        toast.error(title, {
          description: (
            <div>
              <p>{description}</p>
              <SkillTechnicalDetails details={technicalDetails} />
            </div>
          ),
          duration: 10000,
        });
        console.error("Acquire Skill failed:", error);
      }
    };

    const handleAddRepo = async (repo: SkillRepo) => {
      try {
        await addRepoMutation.mutateAsync(repo);
        // Await discovery so we can report the real count
        const { data: freshSkills } = await refetchDiscoverable();
        const count =
          freshSkills?.filter(
            (s) =>
              s.repoOwner === repo.owner &&
              s.repoName === repo.name &&
              (s.repoBranch || "main") === (repo.branch || "main"),
          ).length ?? 0;
        toast.success(
          t("skills.repo.addSuccess", {
            owner: repo.owner,
            name: repo.name,
            count,
          }),
          { closeButton: true },
        );
      } catch (error) {
        showSkillErrorToast(t, "skills.repo.addFailed", error);
      }
    };

    const handleRemoveRepo = async (owner: string, name: string) => {
      try {
        await removeRepoMutation.mutateAsync({ owner, name });
        toast.success(t("skills.repo.removeSuccess", { owner, name }), {
          closeButton: true,
        });
      } catch (error) {
        showSkillErrorToast(t, "skills.repo.removeFailed", error);
      }
    };

    // 过滤技能列表（仓库模式）
    const filteredSkills = useMemo(() => {
      // 按仓库筛选
      const byRepo = skills.filter((skill) => {
        if (filterRepo === "all") return true;
        const skillRepo = `${skill.repoOwner}/${skill.repoName}`;
        return skillRepo === filterRepo;
      });

      // 按安装状态筛选
      const byStatus = byRepo.filter((skill) => {
        if (filterStatus === "acquired") return skill.acquired;
        if (filterStatus === "available") return !skill.acquired;
        return true;
      });

      // 按搜索关键词筛选
      if (!searchQuery.trim()) return byStatus;

      const query = searchQuery.toLowerCase();
      return byStatus.filter((skill) => {
        const name = skill.name?.toLowerCase() || "";
        const repo =
          skill.repoOwner && skill.repoName
            ? `${skill.repoOwner}/${skill.repoName}`.toLowerCase()
            : "";

        return name.includes(query) || repo.includes(query);
      });
    }, [skills, searchQuery, filterRepo, filterStatus]);

    // 是否有更多 skills.sh 结果
    const hasMoreSkillsSh =
      skillsShResult && accumulatedResults.length < skillsShResult.totalCount;
    const searchingSkillsSh =
      (loadingSkillsSh || fetchingSkillsSh) && accumulatedResults.length === 0;

    useEffect(() => {
      onSourceChange?.(effectiveSource);
    }, [effectiveSource, onSourceChange]);

    return (
      <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden bg-background/50">
        {/* 技能网格（可滚动详情区域） */}
        <div className="flex-1 overflow-y-auto overflow-x-hidden animate-fade-in">
          <div className="py-4">
            {/* 搜索来源切换 + 搜索框 */}
            <div className="mb-6 flex flex-col gap-3 md:flex-row md:items-center">
              {/* 来源切换 */}
              <div className="inline-flex gap-1 rounded-md border border-border-default bg-background p-1 shrink-0">
                <Button
                  type="button"
                  size="sm"
                  variant={effectiveSource === "repos" ? "default" : "ghost"}
                  className={
                    effectiveSource === "repos"
                      ? "shadow-sm min-w-[64px]"
                      : "text-muted-foreground hover:text-foreground hover:bg-muted min-w-[64px]"
                  }
                  onClick={() => setSearchSource("repos")}
                >
                  {t("skills.searchSource.repos")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant={effectiveSource === "skillssh" ? "default" : "ghost"}
                  className={
                    effectiveSource === "skillssh"
                      ? "shadow-sm min-w-[80px]"
                      : "text-muted-foreground hover:text-foreground hover:bg-muted min-w-[80px]"
                  }
                  onClick={() => setSearchSource("skillssh")}
                >
                  skills.sh
                </Button>
              </div>

              {effectiveSource === "repos" ? (
                <>
                  {/* 仓库模式搜索框 */}
                  <div className="relative flex-1 min-w-0">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                    <Input
                      type="text"
                      placeholder={t("skills.searchPlaceholder")}
                      value={searchQuery}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      className="pl-9 pr-3"
                    />
                  </div>
                  {/* 仓库筛选 */}
                  <div className="w-full md:w-56">
                    <Select value={filterRepo} onValueChange={setFilterRepo}>
                      <SelectTrigger className="bg-card border shadow-sm text-foreground">
                        <SelectValue
                          placeholder={t("skills.filter.repo")}
                          className="text-left truncate"
                        />
                      </SelectTrigger>
                      <SelectContent className="bg-card text-foreground shadow-lg max-h-64 min-w-[var(--radix-select-trigger-width)]">
                        <SelectItem
                          value="all"
                          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                        >
                          {t("skills.filter.allRepos")}
                        </SelectItem>
                        {repoOptions.map((repo) => (
                          <SelectItem
                            key={repo}
                            value={repo}
                            className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                            title={repo}
                          >
                            <span className="truncate block max-w-[200px]">
                              {repo}
                            </span>
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  {/* 获取状态筛选 */}
                  <div className="w-full md:w-36">
                    <Select
                      value={filterStatus}
                      onValueChange={(val) =>
                        setFilterStatus(val as "all" | "acquired" | "available")
                      }
                    >
                      <SelectTrigger className="bg-card border shadow-sm text-foreground">
                        <SelectValue
                          placeholder={t("skills.filter.placeholder")}
                          className="text-left"
                        />
                      </SelectTrigger>
                      <SelectContent className="bg-card text-foreground shadow-lg">
                        <SelectItem
                          value="all"
                          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                        >
                          {t("skills.filter.all")}
                        </SelectItem>
                        <SelectItem
                          value="acquired"
                          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                        >
                          {t("skills.library.acquired")}
                        </SelectItem>
                        <SelectItem
                          value="available"
                          className="text-left pr-3 [&[data-state=checked]>span:first-child]:hidden"
                        >
                          {t("skills.library.notAcquired")}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  {searchQuery && (
                    <p className="mt-2 text-sm text-muted-foreground">
                      {t("skills.count", { count: filteredSkills.length })}
                    </p>
                  )}
                </>
              ) : (
                <>
                  {/* skills.sh 搜索框 */}
                  <div className="relative flex-1 min-w-0">
                    <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                    <Input
                      type="text"
                      placeholder={t("skills.skillssh.searchPlaceholder")}
                      value={skillsShInput}
                      onChange={(e) => setSkillsShInput(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleSkillsShSearch();
                      }}
                      className="pl-9 pr-3"
                    />
                  </div>
                  <Button
                    size="sm"
                    onClick={handleSkillsShSearch}
                    disabled={
                      skillsShInput.trim().length < 2 || fetchingSkillsSh
                    }
                    className="shrink-0"
                  >
                    {fetchingSkillsSh ? (
                      <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                    ) : (
                      <Search className="h-3.5 w-3.5 mr-1.5" />
                    )}
                    {t("skills.search")}
                  </Button>
                </>
              )}
            </div>

            {/* 内容区域 */}
            {effectiveSource === "repos" ? (
              /* ===== 仓库模式 ===== */
              loading ? (
                <div className="flex items-center justify-center h-64">
                  <RefreshCw className="h-8 w-8 animate-spin text-muted-foreground" />
                </div>
              ) : skills.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-64 text-center">
                  <p className="text-lg font-medium text-foreground">
                    {t("skills.empty")}
                  </p>
                  <p className="mt-2 text-sm text-muted-foreground">
                    {t("skills.emptyDescription")}
                  </p>
                  <Button
                    variant="link"
                    onClick={() => setRepoManagerOpen(true)}
                    className="mt-3 text-sm font-normal"
                  >
                    {t("skills.addRepo")}
                  </Button>
                </div>
              ) : filteredSkills.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-48 text-center">
                  <p className="text-lg font-medium text-foreground">
                    {t("skills.noResults")}
                  </p>
                  <p className="mt-2 text-sm text-muted-foreground">
                    {t("skills.emptyDescription")}
                  </p>
                </div>
              ) : (
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                  {filteredSkills.map((skill) => (
                    <SkillCard
                      key={skill.key}
                      skill={skill}
                      onAcquire={handleAcquire}
                    />
                  ))}
                </div>
              )
            ) : (
              /* ===== skills.sh 模式 ===== */
              <>
                {searchingSkillsSh ? (
                  <div className="flex items-center justify-center h-64">
                    <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
                    <span className="ml-3 text-sm text-muted-foreground">
                      {t("skills.skillssh.loading")}
                    </span>
                  </div>
                ) : skillsShQuery.length < 2 ? (
                  <div className="flex flex-col items-center justify-center h-64 text-center">
                    <Search className="h-12 w-12 text-muted-foreground/30 mb-4" />
                    <p className="text-sm text-muted-foreground">
                      {t("skills.skillssh.searchPlaceholder")}
                    </p>
                  </div>
                ) : accumulatedResults.length === 0 ? (
                  <div className="flex flex-col items-center justify-center h-48 text-center">
                    <p className="text-lg font-medium text-foreground">
                      {t("skills.skillssh.noResults", {
                        query: skillsShQuery,
                      })}
                    </p>
                  </div>
                ) : (
                  <>
                    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                      {accumulatedResults.map((skill) => {
                        const acquired = isSkillsShAcquired(skill);
                        return (
                          <SkillCard
                            key={skill.key}
                            skill={{
                              ...toDiscoverableSkill(skill),
                              acquired,
                            }}
                            installs={skill.installs}
                            onAcquire={handleAcquire}
                          />
                        );
                      })}
                    </div>

                    {/* 加载更多 + 底部信息 */}
                    <div className="mt-6 flex flex-col items-center gap-2">
                      {hasMoreSkillsSh && (
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={fetchingSkillsSh}
                          onClick={() =>
                            setSkillsShOffset(
                              (prev) => prev + SKILLSSH_PAGE_SIZE,
                            )
                          }
                        >
                          {fetchingSkillsSh ? (
                            <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                          ) : null}
                          {t("skills.skillssh.loadMore")}
                        </Button>
                      )}
                      <p className="text-xs text-muted-foreground">
                        {t("skills.skillssh.poweredBy")}
                      </p>
                    </div>
                  </>
                )}
              </>
            )}
          </div>
        </div>

        <Dialog
          open={collision !== null}
          onOpenChange={() => setCollision(null)}
        >
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{t("skills.library.collisionTitle")}</DialogTitle>
              <DialogDescription>
                {t("skills.library.collisionDescription", {
                  directory: collision?.skill.directory,
                })}
              </DialogDescription>
            </DialogHeader>
            <Input
              value={uniqueDirectory}
              aria-label={t("skills.library.directory")}
              onChange={(event) => setUniqueDirectory(event.target.value)}
            />
            <DialogFooter>
              <Button variant="outline" onClick={() => setCollision(null)}>
                {t("common.cancel")}
              </Button>
              <Button
                disabled={!uniqueDirectory.trim() || acquireMutation.isPending}
                onClick={async () => {
                  if (!collision) return;
                  const pending = collision;
                  try {
                    await acquireSkill(
                      pending.skill,
                      pending.sourceKind,
                      uniqueDirectory.trim(),
                    );
                    setCollision(null);
                  } catch (error) {
                    showSkillErrorToast(
                      t,
                      "skills.library.acquireFailed",
                      error,
                    );
                  }
                }}
              >
                {t("skills.library.acquire")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* 仓库管理面板 */}
        {repoManagerOpen && (
          <RepoManagerPanel
            repos={repos}
            skills={skills}
            onAdd={handleAddRepo}
            onRemove={handleRemoveRepo}
            onClose={() => setRepoManagerOpen(false)}
          />
        )}
      </div>
    );
  },
);

SkillsPage.displayName = "SkillsPage";
