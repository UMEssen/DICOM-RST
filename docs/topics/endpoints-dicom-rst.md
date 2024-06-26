# DICOM-RST Endpoints

%product% provides additional features that are not part of the DICOMweb specification.

<api-doc openapi-path="../resources/openapi.yaml" tag="DICOM-RST">
    <api-endpoint endpoint="/aets" method="GET">
        <response type="200">
            <sample lang="JSON" title="Example Response">
            ["RESEARCH", "WILD-WEST"]
            </sample>
        </response>
    </api-endpoint>
    <api-endpoint endpoint="/aets/{aet}" method="GET">
        <response type="200">
            <sample>
            Connection to RESEARCH is healthy.
            </sample>
        </response>
    </api-endpoint>
</api-doc>

<resource src="openapi.yaml" />