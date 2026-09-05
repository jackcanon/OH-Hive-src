export interface AboutInfo {
  app_version: string;
  core_version: string;
  made_by: string;
  made_by_url: string;
  blog_name: string;
  blog_url: string;
}

/**
 * Quiet About block for Preferences/Settings. House rule for every Happy Jack
 * Media app: credit HJM and link to This Is Not A Draft. Used by the desktop
 * app (Preferences → About) and the web app (Settings → About).
 */
export function AboutSection({ info }: { info: AboutInfo }) {
  return (
    <section style={{ color: "#555", fontSize: 13, lineHeight: 1.6 }}>
      <p style={{ margin: 0 }}>
        OH Hive {info.app_version} · core {info.core_version}
      </p>
      <p style={{ margin: 0 }}>
        Made by{" "}
        <a href={info.made_by_url} target="_blank" rel="noreferrer">
          {info.made_by}
        </a>
        . Notes from the workshop at{" "}
        <a href={info.blog_url} target="_blank" rel="noreferrer">
          {info.blog_name}
        </a>
        .
      </p>
    </section>
  );
}
