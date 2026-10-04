import { toast } from 'vue-sonner'

export type AppToastType = 'success' | 'info' | 'warning' | 'error'

interface DeleteConfirmation {
  id: string
  title: string
  description: string
}

export function useAppToast() {
  function notify(type: AppToastType, title: string, description: string) {
    toast[type](title, {
      description,
      duration: 4000,
      id: `${type}:${title}`,
    })
  }

  function confirmDelete({ id, title, description }: DeleteConfirmation) {
    return new Promise<boolean>((resolve) => {
      let settled = false
      let toastId: string | number
      const finish = (confirmed: boolean) => {
        if (settled) return
        settled = true
        resolve(confirmed)
        toast.dismiss(toastId)
      }
      toastId = toast.warning(title, {
        id: `confirm-delete:${id}`,
        description,
        duration: Number.POSITIVE_INFINITY,
        important: true,
        closeButton: true,
        closeButtonPosition: 'top-right',
        classes: { actionButton: 'toast-danger-action' },
        action: { label: '确认删除', onClick: () => finish(true) },
        cancel: { label: '取消', onClick: () => finish(false) },
        onDismiss: () => finish(false),
        onAutoClose: () => finish(false),
      })
    })
  }

  return { notify, confirmDelete }
}
