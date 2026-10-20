import { Link } from "react-router";
import { Thumb } from "@/components/Thumb";
import { useT } from "@/lib/i18n";
import { cardSize } from "@/lib/layout";
import { useProjectContext } from "@/lib/project";
import { useScreenTheme } from "@/lib/theme";
import { Empty } from "./Screens";

export function Components() {
  const t = useT();
  const { key, manifest, captureOf, commentsOn } = useProjectContext();
  const [theme] = useScreenTheme();
  if (!manifest) return <Empty text={t("web.common.loading")} />;
  if (manifest.stories.length === 0) return <Empty text={t("web.components.empty")} />;
  const groups = new Map<string, typeof manifest.stories>();
  for (const story of manifest.stories)
    groups.set(story.component, [...(groups.get(story.component) ?? []), story]);
  return (
    <div className="flex-1 overflow-auto bg-canvas p-6">
      <div className="flex flex-col gap-8">
        {[...groups].map(([component, stories]) => (
          <section key={component} className="flex flex-col gap-3">
            <h2 className="font-mono text-sm font-semibold">{`<${component}>`}</h2>
            <div className="flex flex-wrap gap-6">
              {stories.map((story) => {
                const size = cardSize(story.size, 140);
                return (
                  <Link
                    key={story.id}
                    to={`/p/${key}/t/story/${story.id}${theme === "dark" ? "?theme=dark" : ""}`}
                    data-testid={`story-${story.id}`}
                    className="flex flex-col gap-2"
                    style={{ width: size.w }}
                  >
                    <div style={{ height: size.h }}>
                      <Thumb
                        projectKey={key}
                        capture={captureOf("story", story.id, theme)}
                        title={story.title}
                        openComments={
                          commentsOn("story", story.id).filter((c) => c.status === "open").length
                        }
                        className="size-full"
                      />
                    </div>
                    <span className="text-sm">{story.title}</span>
                  </Link>
                );
              })}
            </div>
          </section>
        ))}
      </div>
    </div>
  );
}
