import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { externalSkillUpdatesApi } from "@/lib/api/externalSkillUpdates";
import type { ExternalSkillInspection } from "@/lib/api/externalSkillUpdates";

export function useExternalSkillUpdates() {
  const client = useQueryClient();
  const inspection = useQuery({
    queryKey: ["skills", "externalUpdates"],
    queryFn: externalSkillUpdatesApi.inspect,
    // Content hashing is expensive. Only explicit CLI reconciliation may scan.
    enabled: false,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const invalidate = async (families: string[]) => {
    // Reconciliation only changes these data families, not migration state.
    await Promise.all(
      families.map((family) =>
        client.invalidateQueries({ queryKey: ["skills", family] }),
      ),
    );
  };
  const linkedFamilies = ["library", "externalUpdates", "activity"];
  const completeCandidate = (candidateId: string) => {
    client.setQueryData<ExternalSkillInspection>(
      ["skills", "externalUpdates"],
      (snapshot) =>
        snapshot && {
          ...snapshot,
          candidates: snapshot.candidates.filter(
            (candidate) => candidate.id !== candidateId,
          ),
        },
    );
  };
  const link = useMutation({
    mutationFn: externalSkillUpdatesApi.link,
    onSuccess: (_, intent) => {
      completeCandidate(intent.candidateId);
      void invalidate(linkedFamilies);
    },
  });
  const apply = useMutation({
    mutationFn: externalSkillUpdatesApi.apply,
    onSuccess: (result, intent) => {
      if (["updated", "up_to_date"].includes(result.outcome)) {
        completeCandidate(intent.candidateId);
      }
      // The mutation is complete; refreshed views must not delay returning to the list.
      void invalidate([
        ...linkedFamilies,
        "deployments",
        "deploymentRecovery",
        "globalSkillImports",
        "projectSkillImports",
      ]);
    },
  });
  return { inspection, link, apply };
}

/** Card-local metadata action: never initializes or refetches the CLI inspector. */
export function useLinkLibrarySkillSource() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: externalSkillUpdatesApi.linkSource,
    onSuccess: async () => {
      await Promise.all([
        client.invalidateQueries({ queryKey: ["skills", "library"] }),
        client.invalidateQueries({ queryKey: ["skills", "externalUpdates"] }),
      ]);
    },
  });
}
