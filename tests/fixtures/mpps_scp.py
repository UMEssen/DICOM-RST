"""Minimal MPPS SCP (PS3.4 Annex F) for the MPPS-RS integration tests.

Keeps all Performed Procedure Steps in memory and supports N-CREATE, N-SET and N-GET.
"""
from pydicom.dataset import Dataset
from pynetdicom import AE, evt
from pynetdicom.sop_class import (
    ModalityPerformedProcedureStep,
    ModalityPerformedProcedureStepRetrieve,
)

steps = {}


def handle_create(event):
    uid = event.request.AffectedSOPInstanceUID
    if uid in steps:
        return 0x0111, None  # Duplicate SOP Instance

    data_set = event.attribute_list
    if data_set.get("PerformedProcedureStepStatus") != "IN PROGRESS":
        return 0x0106, None  # Invalid Attribute Value

    data_set.SOPClassUID = ModalityPerformedProcedureStep
    data_set.SOPInstanceUID = uid
    steps[uid] = data_set
    return 0x0000, data_set


def handle_set(event):
    uid = event.request.RequestedSOPInstanceUID
    if uid not in steps:
        return 0x0112, None  # No Such SOP Instance

    data_set = steps[uid]
    if data_set.PerformedProcedureStepStatus != "IN PROGRESS":
        # PS3.4 F.7.2.2.4
        status = Dataset()
        status.Status = 0x0110
        status.ErrorComment = "Performed Procedure Step Object may no longer be updated"
        status.ErrorID = 0xA710
        return status, None

    data_set.update(event.modification_list)
    return 0x0000, data_set


def handle_get(event):
    uid = event.request.RequestedSOPInstanceUID
    if uid not in steps:
        return 0x0112, None  # No Such SOP Instance

    data_set = steps[uid]
    attributes = event.request.AttributeIdentifierList
    if not attributes:
        return 0x0000, data_set

    if not isinstance(attributes, list):
        attributes = [attributes]
    subset = Dataset()
    for tag in attributes:
        if tag in data_set:
            subset[tag] = data_set[tag]
    return 0x0000, subset


ae = AE(ae_title="MPPS-SCP")
ae.add_supported_context(ModalityPerformedProcedureStep)
ae.add_supported_context(ModalityPerformedProcedureStepRetrieve)
print("MPPS SCP started", flush=True)
ae.start_server(
    ("0.0.0.0", 11112),
    block=True,
    evt_handlers=[
        (evt.EVT_N_CREATE, handle_create),
        (evt.EVT_N_SET, handle_set),
        (evt.EVT_N_GET, handle_get),
    ],
)
