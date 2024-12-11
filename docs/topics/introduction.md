# Introduction

%product% is a robust DICOMweb-compatible gateway server that supports QIDO-RS, WADO-RS and STOW-RS independently of the
PACS vendor, ensuring robust and performant transfers of large amounts of imaging data with high parallelism from
multiple PACS to multiple clients.

This project is part of
the [Open Medical Inference (OMI)](https://www.medizininformatik-initiative.de/de/omi-open-medical-inference) project
and is funded by the German Federal Ministry of Education and Research (BMBF)
with the funding code 01ZZ2315A-P.

The OMI methodology platform aims to
improve the quality of medical diagnoses and treatment decisions by using
artificial intelligence (AI) to simplify time-consuming and repetitive tasks in medicine. To improve medical care, OMI
is developing an open protocol for data exchange on the common framework of the Medical Informatics Initiative (MII).
The project team is also actively involved in the MII interoperability working group.

OMI uses innovative methods to make AI models remotely usable for different hospitals. For example, the project is
creating the technical requirements for a hospital to be able to use the AI of other hospitals to analyze image data -
without having to keep it in its own data center. The semantically interoperable exchange of multimodal healthcare data
is also to be facilitated. OMI is particularly focused on image-based multimodal AI models, which have the potential to
achieve significant progress in the field of medical research and care.

<img src="mii-omi.svg" alt="MII-Logo" width="128" style="inline"/>
<img src="omi-logo.png" alt="OMI-Logo" width="128" style="inline"/>

This documentation provides a reference for the configuration file, the provided endpoints and a user guide for
installation
and troubleshooting.

## Features

- Support for multiple PACS with parallel processing
- Support for basic DICOMweb services:
    - WADO-RS
    - QIDO-RS
    - STOW-RS
- Provides workarounds for common issues with legacy PACS
- Multiple backend implementations:
    - DIMSE
    - S3 (experimental)

The following features are planned:

- De-identification module
- Transcoding module
- Rendering module
- Proxy Backend
