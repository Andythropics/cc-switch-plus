import { useCallback, useEffect } from "react";
import {
  useMutation,
  useInfiniteQuery,
  useQuery,
  useQueryClient,
  keepPreviousData,
  type InfiniteData,
} from "@tanstack/react-query";
import {
  skillsApi,
  type DiscoverableSkill,
  type LibrarySkill,
  type RemoteLibrarySourceKind,
  type DeploymentBatch,
  type DeploymentBatchResult,
  type DeploymentInspectionResult,
  type DeploymentQuery,
  type DeploymentRecoveryInspectionResult,
  type DeploymentRecoveryQuery,
  type SkillsMigrationPreflight,
  type SkillsMigrationExecutionResult,
  type SkillsMigrationIntent,
  type SkillsMigrationRevealIntent,
  type LibrarySkillDeletionInspection,
  type LibrarySkillDeletionIntent,
  type LibrarySkillDeletionResult,
  type LibrarySkillUpdateCheckResult,
  type LibrarySkillUpdateIntent,
  type LibrarySkillUpdateResult,
  type SkillActivityCursor,
  type SkillActivityPage,
  type SkillActivityQuery,
  type SkillsShSearchResult,
} from "@/lib/api/skills";
import {
  projectWorkspacesApi,
  type ProjectWorkspace,
  type WorkspaceRegistration,
  type WorkspaceRegistrationScan,
  type ProjectSkillImportInspection,
  type ProjectSkillImportIntent,
  type ProjectSkillImportResult,
} from "@/lib/api/projectWorkspaces";

/** Private Library snapshots; no consumer deployment state is mixed in. */
export function useLibrarySkills() {
  return useQuery({
    queryKey: ["skills", "library"],
    queryFn: () => skillsApi.getLibrary(),
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
}

/** Desired + observed deployment state for the selected global target. */
export function useSkillDeployments(query?: DeploymentQuery) {
  const deploymentQuery = useQuery<DeploymentInspectionResult>({
    queryKey: ["skills", "deployments", query ?? {}],
    queryFn: () => skillsApi.inspectDeployments(query),
    staleTime: 0,
    // The Tauri window focus event is not always surfaced through React
    // Query's browser focus manager. Reconcile explicitly below so desktop
    // focus regain has the same semantics as browser focus.
    refetchOnWindowFocus: false,
    placeholderData: keepPreviousData,
  });

  useEffect(() => {
    const handleWindowFocus = () => {
      void deploymentQuery.refetch();
    };
    window.addEventListener("focus", handleWindowFocus);
    return () => window.removeEventListener("focus", handleWindowFocus);
  }, [deploymentQuery.refetch]);

  return deploymentQuery;
}

/** Read-only recovery proposals scoped by stable Consumer/Workspace identity. */
export function useDeploymentRecovery(query?: DeploymentRecoveryQuery) {
  const recoveryQuery = useQuery<DeploymentRecoveryInspectionResult>({
    queryKey: ["skills", "deploymentRecovery", query ?? {}],
    queryFn: () => skillsApi.inspectDeploymentRecovery(query),
    staleTime: 0,
    refetchOnWindowFocus: false,
    placeholderData: keepPreviousData,
  });

  useEffect(() => {
    const handleWindowFocus = () => {
      void recoveryQuery.refetch();
    };
    window.addEventListener("focus", handleWindowFocus);
    return () => window.removeEventListener("focus", handleWindowFocus);
  }, [recoveryQuery.refetch]);

  return recoveryQuery;
}

/** Read-only macOS legacy inventory, enabled only after entering Skills. */
export function useSkillsMigrationPreflight({ enabled }: { enabled: boolean }) {
  const preflightQuery = useQuery<SkillsMigrationPreflight>({
    queryKey: ["skills", "migrationPreflight"],
    queryFn: () => skillsApi.inspectSkillsMigrationPreflight(),
    enabled,
    staleTime: 0,
    refetchOnWindowFocus: false,
    placeholderData: keepPreviousData,
  });

  useEffect(() => {
    if (!enabled) return;
    const handleWindowFocus = () => {
      void preflightQuery.refetch();
    };
    window.addEventListener("focus", handleWindowFocus);
    return () => window.removeEventListener("focus", handleWindowFocus);
  }, [enabled, preflightQuery.refetch]);

  return preflightQuery;
}

const skillsMigrationAffectedQueryKeys = [
  ["skills", "migrationPreflight"],
  ["skills", "library"],
  ["skills", "installed"],
  ["skills", "deployments"],
  ["skills", "deploymentRecovery"],
  ["skills", "unmanaged"],
  ["skills", "activity"],
  ["skills", "backups"],
] as const;

function invalidateSkillsMigrationQueries(
  queryClient: ReturnType<typeof useQueryClient>,
) {
  return Promise.all(
    skillsMigrationAffectedQueryKeys.map((queryKey) =>
      queryClient.invalidateQueries({ queryKey: [...queryKey] }),
    ),
  );
}

export function useApplySkillsMigration() {
  const queryClient = useQueryClient();
  return useMutation<
    SkillsMigrationExecutionResult,
    Error,
    SkillsMigrationIntent
  >({
    mutationFn: (intent) => skillsApi.applySkillsMigration(intent),
    onSettled: () => invalidateSkillsMigrationQueries(queryClient),
  });
}

export function useRevealSkillsMigrationPlanItem() {
  return useMutation<boolean, Error, SkillsMigrationRevealIntent>({
    mutationFn: (intent) => skillsApi.revealSkillsMigrationPlanItem(intent),
  });
}

export function useResumeSkillsMigration() {
  const queryClient = useQueryClient();
  return useMutation<SkillsMigrationExecutionResult, Error, void>({
    mutationFn: () => skillsApi.resumeSkillsMigration(),
    onSettled: () => invalidateSkillsMigrationQueries(queryClient),
  });
}

export function useRestoreSkillsMigrationBackup() {
  const queryClient = useQueryClient();
  return useMutation<SkillsMigrationExecutionResult, Error, string>({
    mutationFn: (backupId) => skillsApi.restoreSkillsMigrationBackup(backupId),
    onSettled: () => invalidateSkillsMigrationQueries(queryClient),
  });
}

/** Redacted, device-local activity entries in backend-provided newest-first order. */
export function useSkillActivity(query?: Omit<SkillActivityQuery, "cursor">) {
  const activityQuery = useInfiniteQuery<
    SkillActivityPage,
    Error,
    InfiniteData<SkillActivityPage>,
    [string, string, Omit<SkillActivityQuery, "cursor">],
    SkillActivityCursor | undefined
  >({
    queryKey: ["skills", "activity", query ?? {}],
    queryFn: ({ pageParam }) =>
      skillsApi.listActivity(
        pageParam === undefined ? query : { ...query, cursor: pageParam },
      ),
    initialPageParam: undefined,
    getNextPageParam: (lastPage) =>
      lastPage.hasMore ? (lastPage.nextCursor ?? undefined) : undefined,
    staleTime: 0,
    refetchOnWindowFocus: false,
  });

  return {
    ...activityQuery,
    entries: activityQuery.data?.pages.flatMap((page) => page.entries) ?? [],
  };
}

/** Reconcile every mounted Global or Project Deployment observation. */
export function useRefreshSkillDeployments() {
  const queryClient = useQueryClient();
  return useCallback(
    () =>
      queryClient.refetchQueries({
        queryKey: ["skills", "deployments"],
        type: "active",
      }),
    [queryClient],
  );
}

export function useApplySkillDeployments() {
  const queryClient = useQueryClient();
  return useMutation<DeploymentBatchResult, Error, DeploymentBatch>({
    mutationFn: (batch) => skillsApi.applyDeployments(batch),
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "deployments"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "activity"] }),
        queryClient.invalidateQueries({
          queryKey: ["skills", "deploymentRecovery"],
        }),
      ]),
  });
}

export function useProjectWorkspaces() {
  return useQuery<ProjectWorkspace[]>({
    queryKey: ["skills", "projectWorkspaces"],
    // Keep archived identities available to the lifecycle section; the
    // Projects UI separates active and archived rows for presentation.
    queryFn: () => projectWorkspacesApi.list(true),
    staleTime: 0,
    placeholderData: keepPreviousData,
  });
}

export function useInspectProjectSkillImports(workspaceId?: string | null) {
  return useQuery<ProjectSkillImportInspection>({
    queryKey: ["skills", "projectSkillImports", workspaceId],
    queryFn: () => projectWorkspacesApi.inspectSkillImports(workspaceId!),
    enabled: Boolean(workspaceId),
    staleTime: 0,
    placeholderData: keepPreviousData,
  });
}

export function useApplyProjectSkillImport() {
  const queryClient = useQueryClient();
  return useMutation<ProjectSkillImportResult, Error, ProjectSkillImportIntent>(
    {
      mutationFn: (intent) => projectWorkspacesApi.applySkillImport(intent),
      onSettled: (_data, _error, intent) =>
        Promise.all([
          queryClient.invalidateQueries({
            queryKey: ["skills", "projectSkillImports", intent?.workspaceId],
          }),
          queryClient.invalidateQueries({ queryKey: ["skills", "library"] }),
          queryClient.invalidateQueries({
            queryKey: ["skills", "deployments"],
          }),
          queryClient.invalidateQueries({ queryKey: ["skills", "activity"] }),
        ]),
    },
  );
}

export function useInspectProjectWorkspace() {
  return useMutation<WorkspaceRegistrationScan, Error, string>({
    mutationFn: (path) => projectWorkspacesApi.inspect(path),
  });
}

export function useRegisterProjectWorkspace() {
  const queryClient = useQueryClient();
  return useMutation<
    WorkspaceRegistration,
    Error,
    { path: string; displayName?: string }
  >({
    mutationFn: ({ path, displayName }) =>
      projectWorkspacesApi.register(path, displayName),
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({
          queryKey: ["skills", "projectWorkspaces"],
        }),
        queryClient.invalidateQueries({ queryKey: ["skills", "activity"] }),
      ]),
  });
}

function useProjectWorkspaceMutation<TVariables, TResult>(
  mutationFn: (variables: TVariables) => Promise<TResult>,
) {
  const queryClient = useQueryClient();
  return useMutation<TResult, Error, TVariables>({
    mutationFn,
    // Lifecycle changes affect both the list and deployment inspection (an
    // archived or relocated root must be re-derived before showing actions).
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({
          queryKey: ["skills", "projectWorkspaces"],
        }),
        queryClient.invalidateQueries({ queryKey: ["skills", "deployments"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "activity"] }),
      ]),
  });
}

export function useRenameProjectWorkspace() {
  return useProjectWorkspaceMutation(
    ({
      workspaceId,
      displayName,
    }: {
      workspaceId: string;
      displayName: string;
    }) => projectWorkspacesApi.rename(workspaceId, displayName),
  );
}

export function useArchiveProjectWorkspace() {
  return useProjectWorkspaceMutation((workspaceId: string) =>
    projectWorkspacesApi.archive(workspaceId),
  );
}

export function useRestoreProjectWorkspace() {
  return useProjectWorkspaceMutation((workspaceId: string) =>
    projectWorkspacesApi.restore(workspaceId),
  );
}

export function useRelocateProjectWorkspace() {
  return useProjectWorkspaceMutation(
    ({ workspaceId, path }: { workspaceId: string; path: string }) =>
      projectWorkspacesApi.relocate(workspaceId, path),
  );
}

export function useForgetProjectWorkspace() {
  return useProjectWorkspaceMutation((workspaceId: string) =>
    projectWorkspacesApi.forget(workspaceId),
  );
}

export function useAcquireLibrarySkill() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      skill,
      sourceKind,
      directoryName,
    }: {
      skill: DiscoverableSkill;
      sourceKind: RemoteLibrarySourceKind;
      directoryName?: string;
    }) => skillsApi.acquireLibrary(skill, sourceKind, directoryName),
    onSuccess: (acquired) => {
      queryClient.setQueryData<LibrarySkill[]>(
        ["skills", "library"],
        (current) =>
          current
            ? [...current.filter((item) => item.id !== acquired.id), acquired]
            : [acquired],
      );
    },
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "library"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "activity"] }),
      ]),
  });
}

export function useAcquireLibrarySkillsFromZip() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      filePath,
      directoryNames = {},
    }: {
      filePath: string;
      directoryNames?: Record<string, string>;
    }) => skillsApi.acquireLibraryFromZip(filePath, directoryNames),
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "library"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "activity"] }),
      ]),
  });
}

export function useUpdateLibrarySkillMetadata() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      displayName,
      description,
    }: {
      id: string;
      displayName: string;
      description?: string;
    }) => skillsApi.updateLibraryMetadata(id, displayName, description),
    onSuccess: (updated) => {
      queryClient.setQueryData<LibrarySkill[]>(
        ["skills", "library"],
        (current) =>
          current?.map((item) => (item.id === updated.id ? updated : item)),
      );
    },
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "library"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "activity"] }),
      ]),
  });
}

/** Check and stage a remote Library snapshot without touching live content. */
export function useCheckLibrarySkillUpdate() {
  const queryClient = useQueryClient();
  return useMutation<LibrarySkillUpdateCheckResult, Error, string>({
    mutationFn: (librarySkillId) =>
      skillsApi.checkLibrarySkillUpdate(librarySkillId),
    onSuccess: (result) => {
      queryClient.setQueryData(
        ["skills", "libraryUpdate", result.librarySkillId],
        result,
      );
    },
  });
}

/** Apply a staged snapshot and reconcile all Library/deployment observations. */
export function useApplyLibrarySkillUpdate() {
  const queryClient = useQueryClient();
  return useMutation<LibrarySkillUpdateResult, Error, LibrarySkillUpdateIntent>(
    {
      mutationFn: (intent) => skillsApi.applyLibrarySkillUpdate(intent),
      onSettled: (_result, _error, intent) =>
        Promise.all([
          queryClient.invalidateQueries({ queryKey: ["skills", "library"] }),
          queryClient.invalidateQueries({
            queryKey: ["skills", "deployments"],
          }),
          queryClient.invalidateQueries({
            queryKey: ["skills", "libraryUpdate", intent.librarySkillId],
          }),
          queryClient.invalidateQueries({ queryKey: ["skills", "activity"] }),
        ]),
    },
  );
}

/** Inspect deployment reachability before a destructive Library delete. */
export function useInspectLibrarySkillDeletion() {
  return useMutation<LibrarySkillDeletionInspection, Error, string>({
    mutationFn: (librarySkillId) =>
      skillsApi.inspectLibrarySkillDeletion(librarySkillId),
  });
}

/** Delete a Library snapshot and invalidate all affected observations. */
export function useDeleteLibrarySkill() {
  const queryClient = useQueryClient();
  return useMutation<
    LibrarySkillDeletionResult,
    Error,
    LibrarySkillDeletionIntent
  >({
    mutationFn: (intent) => skillsApi.deleteLibrarySkill(intent),
    onSettled: (_result, _error, intent) =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "library"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "deployments"] }),
        queryClient.invalidateQueries({
          queryKey: ["skills", "libraryUpdate", intent.librarySkillId],
        }),
        queryClient.invalidateQueries({ queryKey: ["skills", "activity"] }),
      ]),
  });
}

/**
 * 发现可安装的 Skills（从仓库获取）
 * 使用 staleTime: Infinity 和 placeholderData: keepPreviousData
 * 实现首次进入使用缓存，只有刷新时才重新获取
 */
export function useDiscoverableSkills() {
  return useQuery({
    queryKey: ["skills", "discoverable"],
    queryFn: () => skillsApi.discoverAvailable(),
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
}

/**
 * 获取仓库列表
 */
export function useSkillRepos() {
  return useQuery({
    queryKey: ["skills", "repos"],
    queryFn: () => skillsApi.getRepos(),
  });
}

/**
 * 添加仓库
 */
export function useAddSkillRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: skillsApi.addRepo,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills", "repos"] });
      queryClient.invalidateQueries({ queryKey: ["skills", "discoverable"] });
    },
  });
}

/**
 * 删除仓库
 */
export function useRemoveSkillRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ owner, name }: { owner: string; name: string }) =>
      skillsApi.removeRepo(owner, name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills", "repos"] });
      queryClient.invalidateQueries({ queryKey: ["skills", "discoverable"] });
    },
  });
}

// ========== skills.sh 搜索 ==========

/**
 * 搜索 skills.sh 公共目录
 * 使用 300ms staleTime 和 keepPreviousData 实现平滑搜索体验
 */
export function useSearchSkillsSh(
  query: string,
  limit: number,
  offset: number,
) {
  return useQuery({
    queryKey: ["skills", "skillssh", query, limit, offset],
    queryFn: () => skillsApi.searchSkillsSh(query, limit, offset),
    enabled: query.length >= 2,
    staleTime: 5 * 60 * 1000,
    placeholderData: keepPreviousData,
  });
}

// ========== 辅助类型 ==========

export type { DiscoverableSkill, SkillsShSearchResult };
