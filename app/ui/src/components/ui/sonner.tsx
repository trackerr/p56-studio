import { useTheme } from "next-themes"
import { Toaster as Sonner } from "sonner"
import { useT } from "@/lib/i18n"

type ToasterProps = React.ComponentProps<typeof Sonner>

const Toaster = ({ toastOptions, ...props }: ToasterProps) => {
  const { theme = "system" } = useTheme()
  const { t } = useT()

  return (
    <Sonner
      theme={theme as ToasterProps["theme"]}
      className="toaster group"
      containerAriaLabel={t("common.notifications")}
      toastOptions={{
        closeButtonAriaLabel: t("common.close"),
        classNames: {
          toast:
            "group toast group-[.toaster]:bg-background group-[.toaster]:text-foreground group-[.toaster]:border-border group-[.toaster]:shadow-lg",
          description: "group-[.toast]:text-muted-foreground",
          actionButton:
            "group-[.toast]:bg-primary group-[.toast]:text-primary-foreground",
          cancelButton:
            "group-[.toast]:bg-muted group-[.toast]:text-muted-foreground",
        },
        ...toastOptions,
      }}
      {...props}
    />
  )
}

export { Toaster }
