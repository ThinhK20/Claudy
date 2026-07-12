export default function OverlayPage() {
  return (
    <div className="flex h-screen items-center justify-center">
      <div className="flex items-center gap-2 rounded-full bg-black/80 px-4 py-2 text-sm text-white">
        <span className="h-2 w-2 rounded-full bg-red-500" />
        Recording…
      </div>
    </div>
  );
}
