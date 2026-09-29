import { Icon, type IconName } from "./Icon";

/** A Home card's title with its icon, and a link to the screen it comes from. */
export default function CardHead({ id, icon, title, href, link }: { id: string; icon: IconName; title: string; href: string; link: string }) {
  return (
    <div className="home-card-head">
      <h2 id={id}>
        <span className="home-card-icon">
          <Icon name={icon} size={18} />
        </span>
        {title}
      </h2>
      <a className="home-link" href={href}>{link}</a>
    </div>
  );
}
