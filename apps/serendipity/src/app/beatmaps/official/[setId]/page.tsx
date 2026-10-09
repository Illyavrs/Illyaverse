import { OfficialBeatmapsetPage } from '@/features/public-official-maps';

export default async function Page({ params }: { params: Promise<{ setId: string }> }) {
  const { setId } = await params;
  return <OfficialBeatmapsetPage setId={setId} />;
}
