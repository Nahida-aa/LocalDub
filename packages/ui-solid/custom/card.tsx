import { Show, type JSX } from "solid-js";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
  cardVariants,
} from "../base/card";
import { cva, type VariantProps } from "class-variance-authority";
export interface CardXProps {
  class?: string;
  title?: string;
  description?: string;
  // Content 槽位刻意关闭: 非 title/description 的内容一律放 Footer。
  // 若 Content 与 Footer 都可选, 卡片主体/底部两个可空槽位会让布局不稳定
  // (何时有主体、何时有底部分歧), 统一收敛到 Footer 保证结构可预测。
  // Content?: JSX.Element;
  Footer?: JSX.Element;
  FooterClass?: string;
}
export const CardX = (p: CardXProps & VariantProps<typeof cardVariants>) => {
  return (
    <Card variant={p.variant} size={p.size} class={p.class}>
      <Show when={p.title || p.description}>
        <CardHeader>
          <Show when={p.title}>{(title) => <CardTitle>{title()}</CardTitle>}</Show>
          <Show when={p.description}>
            {(description) => <CardDescription>{description()}</CardDescription>}
          </Show>
        </CardHeader>
      </Show>
      {/* Content 槽位刻意关闭: 非 title/description 的内容一律放 Footer。
        若 Content 与 Footer 都可选, 卡片主体/底部两个可空槽位会让布局不稳定,
        统一收敛到 Footer 保证结构可预测。 */}
      {/* <Show when={p.Content}>
      {(Content) => <CardContent>
        {Content()}
      </CardContent>}
    </Show> */}
      <Show when={p.Footer}>
        {(Footer) => <CardFooter class={p.FooterClass}>{Footer()}</CardFooter>}
      </Show>
    </Card>
  );
};
