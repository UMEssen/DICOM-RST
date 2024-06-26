# DICOMweb Endpoints

%product% implements the basic DICOMweb services such as QIDO-RS, WADO-RS and STOW-RS.

Please note that all DICOMweb endpoints are prefixed with the AE title.
If you want to call the search service for the AET <code>RESEARCH</code>, prepend <code>/aets/RESEARCH</code>:
<code-block lang="http">
GET http://localhost:8080/aets/RESEARCH/studies
</code-block>

## QIDO-RS

<api-doc openapi-path="../resources/openapi.yaml" tag="QIDO-RS">
    <api-endpoint endpoint="/aets/{aet}/studies" method="GET">
        <request>
            <sample lang="JSON">PACS</sample>
        </request>
        <response type="200">
            <sample lang="JSON">
            [ 
                { 
                    "0020000D": { 
                        "vr": "UI", 
                        "Value": [ "1.2.3" ] 
                    } 
                }, 
                { 
                    "0020000D": { 
                        "vr": "UI", 
                        "Value": [ "4.5.6" ] 
                    } 
                } 
            ]
            </sample>
        </response>
    </api-endpoint>

</api-doc>

## WADO-RS

<api-doc openapi-path="../resources/openapi.yaml" tag="WADO-RS"/>

## STOW-RS

<api-doc openapi-path="../resources/openapi.yaml" tag="STOW-RS">
    <api-endpoint endpoint="/aets/{aet}/studies" method="POST">
        <response type="200">
            <sample lang="JSON">
            {
                "00081199": {
                    "vr": "SQ",
                    ”Value": [
                        {
                            "00081155": {
                                "vr": "UI",
                                "Value": [ "1.2.3.4.5.6.7.8.9" ]
                            }
                        }
                    ]
                }
            }
            </sample>
        </response>
    </api-endpoint>
</api-doc>

<resource src="openapi.yaml" />
