use twin_k8s::kubeconfig_check;

use crate::model::DoctorK8s;

pub(crate) fn host() -> DoctorK8s {
    let check = kubeconfig_check();
    let detail = if check.present {
        "kubeconfig found. Live cluster reads are not built; set TWIN_K8S_FIXTURE to scan list documents. Read-only get/list only."
    } else {
        "kubeconfig not found. Set TWIN_K8S_FIXTURE to scan list documents. Read-only get/list only."
    };
    DoctorK8s {
        kubeconfig_present: check.present,
        kubeconfig_path: check.path,
        detail: detail.to_string(),
    }
}
