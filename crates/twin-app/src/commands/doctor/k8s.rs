use twin_k8s::kubeconfig_check;

use crate::model::DoctorK8s;

pub(crate) fn host() -> DoctorK8s {
    let check = kubeconfig_check();
    let detail = if check.present {
        "kubeconfig found. twin k8s scan reads the cluster with bounded get/list requests; --fixture DIR selects list documents. External credential commands are disabled."
    } else {
        "kubeconfig not found. In-cluster credentials or twin k8s scan --fixture DIR can provide coverage. Read-only get/list only."
    };
    DoctorK8s {
        kubeconfig_present: check.present,
        kubeconfig_path: check.path,
        detail: detail.to_string(),
    }
}
